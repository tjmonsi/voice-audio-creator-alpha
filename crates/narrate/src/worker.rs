//! Client side of the worker protocol (spec-narrate-worker.md).
//!
//! One session is one worker process. Requests go out as JSON lines on stdin; replies come back on
//! stdout and are read by a thread so the caller can watch the interrupt flag and time limits.

use std::ffi::OsString;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use crate::ctx::Ctx;
use crate::datadir::DataDir;
use crate::fsutil::write_atomic;
use crate::interrupt;
use crate::logsink::LogSink;
use crate::pins::{WORKER_SCRIPT, worker_script_dir_name};
use crate::proc::{
    self, LineEvent, MAX_LINE_BYTES, Spawn, read_limited_line, set_env, wait_for_exit,
};

/// Which environment a worker runs in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Everything local: `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`.
    Offline,
    /// The setup `models` step: Hugging Face downloads allowed.
    Download,
}

/// Where the worker's stderr lines are recorded.
pub enum StderrTarget {
    /// Printed with `--verbose` only (the `status` probe).
    Discard,
    /// `<data>\logs\worker-<pid>.log` (generation).
    PerWorkerLog,
    /// A log shared with the rest of the command (setup).
    Shared(Arc<LogSink>),
}

/// Why a request did not produce a success reply.
#[derive(Debug)]
pub enum WorkerError {
    /// The process could not start or never answered `hello`.
    Start(String),
    /// The worker answered with `ok: false`.
    Op {
        code: String,
        message: String,
        traceback: Option<String>,
    },
    /// The worker's stdout closed before it replied.
    Exited { code: Option<i32> },
    /// A line was not a valid reply.
    Protocol(String),
    /// No reply within the time limit.
    Timeout,
    /// Ctrl+C or Ctrl+Break arrived while waiting.
    Interrupted,
}

pub struct WorkerSession {
    child: Child,
    stdin: Option<ChildStdin>,
    events: Receiver<LineEvent>,
    threads: Vec<JoinHandle<()>>,
    next_id: u64,
    log_path: Option<PathBuf>,
    job: Arc<crate::proc::Job>,
}

/// Resolve the program and arguments for a worker, writing the embedded script when needed.
pub fn worker_program(ctx: &Ctx) -> std::io::Result<(PathBuf, Vec<OsString>)> {
    if let Some(cmd) = &ctx.hooks.worker_cmd {
        return Ok((cmd.clone(), Vec::new()));
    }
    let script = ctx
        .data
        .worker_dir()
        .join(worker_script_dir_name())
        .join("narrate_worker.py");
    if !script.is_file() {
        write_atomic(&script, WORKER_SCRIPT.as_bytes())?;
    }
    let args = ["-I", "-X", "utf8", "-u"]
        .map(OsString::from)
        .into_iter()
        .chain([script.into_os_string()])
        .collect();
    Ok((ctx.data.venv_python(), args))
}

/// The environment of a worker child (spec-narrate-setup.md, Child process environment).
pub fn worker_env(data: &DataDir, run_tmp: &Path, mode: Mode) -> Vec<(OsString, OsString)> {
    let mut env = proc::base_env();
    set_env(&mut env, "TEMP", run_tmp);
    set_env(&mut env, "TMP", run_tmp);
    let hf = data.hf_home();
    set_env(&mut env, "HF_HOME", &hf);
    set_env(&mut env, "HF_HUB_CACHE", hf.join("hub"));
    set_env(&mut env, "HF_XET_CACHE", hf.join("xet"));
    set_env(&mut env, "HF_ASSETS_CACHE", hf.join("assets"));
    for (name, value) in [
        ("HF_HUB_DISABLE_TELEMETRY", "1"),
        ("DO_NOT_TRACK", "1"),
        ("HF_HUB_DISABLE_PROGRESS_BARS", "1"),
        ("HF_HUB_DISABLE_SYMLINKS_WARNING", "1"),
        ("HF_HUB_DISABLE_XET", "1"),
        ("TQDM_DISABLE", "1"),
    ] {
        set_env(&mut env, name, value);
    }
    set_env(&mut env, "TORCH_HOME", data.root().join("torch-home"));
    set_env(&mut env, "NUMBA_CACHE_DIR", data.root().join("numba-cache"));
    set_env(&mut env, "CUDA_CACHE_PATH", data.root().join("cuda-cache"));
    set_env(&mut env, "MPLCONFIGDIR", data.root().join("mpl-config"));
    if mode == Mode::Offline {
        set_env(&mut env, "HF_HUB_OFFLINE", "1");
        set_env(&mut env, "TRANSFORMERS_OFFLINE", "1");
    }
    env.extend(proc::worker_hook_env());
    env
}

impl WorkerSession {
    /// Start the worker, assign it to the job, and complete the `hello` exchange (60 s limit).
    pub fn start(
        ctx: &Ctx,
        run_tmp: &Path,
        mode: Mode,
        target: StderrTarget,
    ) -> Result<WorkerSession, WorkerError> {
        let (program, args) =
            worker_program(ctx).map_err(|err| WorkerError::Start(err.to_string()))?;
        let spec = Spawn {
            program: &program,
            args,
            cwd: Some(ctx.data.root()),
            env: worker_env(&ctx.data, run_tmp, mode),
            pipe_stdin: true,
        };
        let (mut child, assignment) =
            proc::spawn(&ctx.job, &spec).map_err(|err| WorkerError::Start(err.to_string()))?;
        let pid = child.id();
        ctx.verbose_line(format_args!("verbose: worker pid {pid}"));
        if let (StderrTarget::Shared(log), true) = (&target, ctx.verbose) {
            log.file_only(&format!("verbose: worker pid {pid}"));
        }
        if let Some(err) = assignment {
            ctx.verbose_line(format_args!(
                "verbose: could not assign worker {pid} to the job object: {err}"
            ));
        }

        let (sink, log_path) = match target {
            StderrTarget::Shared(sink) => (sink, None),
            StderrTarget::Discard => (Arc::new(LogSink::new(None, ctx.verbose)), None),
            StderrTarget::PerWorkerLog => {
                let path = ctx.data.logs().join(format!("worker-{pid}.log"));
                let file = std::fs::create_dir_all(ctx.data.logs())
                    .and_then(|()| File::create(&path))
                    .ok();
                (Arc::new(LogSink::new(file, ctx.verbose)), Some(path))
            }
        };
        let (tx, events) = mpsc::channel();
        let mut threads = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            threads.push(thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let event = read_limited_line(&mut reader, MAX_LINE_BYTES);
                    let last = !matches!(event, LineEvent::Line(_));
                    if tx.send(event).is_err() || last {
                        break;
                    }
                }
            }));
        }
        if let Some(stderr) = child.stderr.take() {
            let in_file_prefix = log_path.is_none();
            threads.push(thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                while let LineEvent::Line(text) = read_limited_line(&mut reader, MAX_LINE_BYTES) {
                    sink.line("worker: ", in_file_prefix, &text);
                }
            }));
        }
        let stdin = child.stdin.take();
        let mut session = WorkerSession {
            child,
            stdin,
            events,
            threads,
            next_id: 1,
            log_path,
            job: Arc::clone(&ctx.job),
        };
        match session.request(
            "hello",
            json!({ "protocol": 1 }),
            Some(Duration::from_secs(60)),
        ) {
            Ok(reply) if reply.get("protocol").and_then(Value::as_u64) == Some(1) => Ok(session),
            Ok(reply) => Err(WorkerError::Protocol(format!(
                "hello answered with protocol {}",
                reply.get("protocol").unwrap_or(&Value::Null)
            ))),
            Err(WorkerError::Timeout) => {
                Err(WorkerError::Start("no hello reply within 60 s".to_owned()))
            }
            Err(other) => Err(other),
        }
    }

    /// The per-worker log file, when this session writes one.
    pub fn log_path(&self) -> Option<&Path> {
        self.log_path.as_deref()
    }

    /// Send one request and wait for its reply; `fields` must be a JSON object.
    pub fn request(
        &mut self,
        op: &str,
        fields: Value,
        limit: Option<Duration>,
    ) -> Result<Value, WorkerError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut object = match fields {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        object.insert("id".to_owned(), json!(id));
        object.insert("op".to_owned(), json!(op));
        let mut line = Value::Object(object).to_string();
        line.push('\n');
        // A write error means the worker is gone; the reader thread reports that as end-of-file.
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = stdin
                .write_all(line.as_bytes())
                .and_then(|()| stdin.flush());
        }
        let deadline = limit.map(|l| Instant::now() + l);
        loop {
            match self.events.recv_timeout(Duration::from_millis(50)) {
                Ok(LineEvent::Line(text)) => return self.parse_reply(id, &text),
                Ok(LineEvent::TooLong) => {
                    return Err(WorkerError::Protocol(
                        "a reply line exceeds 32 MiB".to_owned(),
                    ));
                }
                Ok(LineEvent::Eof | LineEvent::Failed) | Err(RecvTimeoutError::Disconnected) => {
                    return Err(self.after_exit());
                }
                Err(RecvTimeoutError::Timeout) => {
                    if interrupt::is_set() {
                        return Err(WorkerError::Interrupted);
                    }
                    if deadline.is_some_and(|d| Instant::now() >= d) {
                        return Err(WorkerError::Timeout);
                    }
                }
            }
        }
    }

    fn parse_reply(&mut self, id: u64, text: &str) -> Result<Value, WorkerError> {
        let reply: Value = serde_json::from_str(text)
            .map_err(|err| WorkerError::Protocol(format!("not JSON ({err})")))?;
        let Some(object) = reply.as_object() else {
            return Err(WorkerError::Protocol(
                "reply is not a JSON object".to_owned(),
            ));
        };
        let reply_id = object.get("id").and_then(Value::as_u64);
        if reply_id != Some(id) {
            return Err(WorkerError::Protocol(format!(
                "reply id {} for request {id}",
                object.get("id").unwrap_or(&Value::Null)
            )));
        }
        match object.get("ok").and_then(Value::as_bool) {
            Some(true) => Ok(reply),
            Some(false) => {
                let error = object.get("error");
                let text_of = |key: &str| {
                    error
                        .and_then(|e| e.get(key))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                };
                Err(WorkerError::Op {
                    code: text_of("code").unwrap_or_else(|| "unknown".to_owned()),
                    message: text_of("message").unwrap_or_default(),
                    traceback: text_of("traceback"),
                })
            }
            None => Err(WorkerError::Protocol("reply has no boolean ok".to_owned())),
        }
    }

    /// RULE-006: after an unexpected exit, give a pending interrupt 500 ms to arrive.
    fn after_exit(&mut self) -> WorkerError {
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if interrupt::is_set() {
                return WorkerError::Interrupted;
            }
            thread::sleep(Duration::from_millis(20));
        }
        if interrupt::is_set() {
            return WorkerError::Interrupted;
        }
        if !wait_for_exit(&mut self.child, Duration::from_secs(2)) {
            self.job.terminate();
            let _ = self.child.wait();
        }
        WorkerError::Exited {
            code: self.child.try_wait().ok().flatten().and_then(|s| s.code()),
        }
    }

    /// Ask the worker to exit, then wait for it (clean end of a session).
    pub fn shutdown(mut self) {
        let _ = self.request("shutdown", json!({}), Some(Duration::from_secs(10)));
        self.finish(Duration::from_secs(5));
    }

    /// Stop the worker without a handshake: close stdin, wait up to `grace`, then terminate it.
    pub fn abort(mut self, grace: Duration) {
        self.finish(grace);
    }

    fn finish(&mut self, grace: Duration) {
        self.stdin = None;
        if !wait_for_exit(&mut self.child, grace) {
            self.job.terminate();
            let _ = self.child.wait();
        }
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }
}

impl Drop for WorkerSession {
    fn drop(&mut self) {
        self.stdin = None;
        if matches!(self.child.try_wait(), Ok(None)) {
            self.job.terminate();
            let _ = self.child.wait();
        }
    }
}

/// Print a `verbose:` line for a worker error that carries a traceback, one per line.
pub fn traceback_details(traceback: &Option<String>) -> Vec<String> {
    traceback
        .as_deref()
        .map(|t| t.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}
