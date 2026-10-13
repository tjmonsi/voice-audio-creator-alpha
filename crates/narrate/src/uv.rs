//! Locating and running uv 0.10.2 (spec-narrate-setup.md, Requirement 1 and Child process
//! environment).

use std::ffi::OsString;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::ctx::Ctx;
use crate::error::CliError;
use crate::interrupt;
use crate::logsink::LogSink;
use crate::proc::{self, LineEvent, MAX_LINE_BYTES, Spawn, read_limited_line, set_env};

/// The only uv version `narrate` accepts.
pub const UV_VERSION: &str = "0.10.2";

/// Find `uv.exe`: the test-hook path, else the first `uv.exe` in the PATH directories, in order.
pub fn locate(ctx: &Ctx) -> Result<PathBuf, CliError> {
    if let Some(path) = &ctx.hooks.uv {
        return if path.is_file() {
            Ok(path.clone())
        } else {
            Err(CliError::uv_not_found(Some(path)))
        };
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path_var)
        .map(|dir| dir.join("uv.exe"))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| CliError::uv_not_found(None))
}

/// Captured result of a uv run.
pub struct UvRun {
    pub success: bool,
    /// Every output line (stdout and stderr), in arrival order.
    pub lines: Vec<String>,
}

/// Run `uv --version` with the inherited `TEMP` and `TMP` and return the version number.
pub fn version(ctx: &Ctx, uv: &Path) -> Result<String, CliError> {
    let run = run_child(
        ctx,
        uv,
        vec![OsString::from("--version")],
        proc::base_env(),
        None,
        None,
    )?;
    let first = run.lines.first().cloned().unwrap_or_default();
    let found = first
        .split_whitespace()
        .nth(1)
        .unwrap_or("unknown")
        .to_owned();
    if !run.success && found == "unknown" {
        return Err(CliError::internal(format!("uv --version failed: {first}")));
    }
    Ok(found)
}

/// Settings for the steps after preflight.
pub struct UvEnv<'a> {
    pub ctx: &'a Ctx,
    pub uv: &'a Path,
    pub run_tmp: &'a Path,
    pub log: Arc<LogSink>,
}

impl UvEnv<'_> {
    /// Run uv with the redirected directories; `sync` adds `UV_PYTHON_DOWNLOADS=never`.
    pub fn run(&self, args: &[&str], sync: bool) -> Result<UvRun, CliError> {
        let data = &self.ctx.data;
        let mut env = proc::base_env();
        set_env(&mut env, "TEMP", self.run_tmp);
        set_env(&mut env, "TMP", self.run_tmp);
        set_env(&mut env, "UV_CACHE_DIR", data.uv_cache());
        set_env(&mut env, "UV_PYTHON_INSTALL_DIR", data.python_dir());
        set_env(&mut env, "UV_PYTHON_CACHE_DIR", data.python_cache());
        set_env(&mut env, "UV_PROJECT_ENVIRONMENT", data.venv());
        set_env(&mut env, "UV_NO_CONFIG", "1");
        if sync {
            set_env(&mut env, "UV_PYTHON_DOWNLOADS", "never");
        }
        run_child(
            self.ctx,
            self.uv,
            args.iter().map(OsString::from).collect(),
            env,
            Some(Arc::clone(&self.log)),
            Some(data.root()),
        )
    }
}

/// Start uv, drain its output, and wait for it while watching the interrupt flag.
fn run_child(
    ctx: &Ctx,
    uv: &Path,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    log: Option<Arc<LogSink>>,
    cwd: Option<&Path>,
) -> Result<UvRun, CliError> {
    let spec = Spawn {
        program: uv,
        args,
        cwd,
        env,
        pipe_stdin: false,
    };
    let (mut child, assignment) = proc::spawn(&ctx.job, &spec)
        .map_err(|err| CliError::internal(format!("cannot start {}: {err}", uv.display())))?;
    ctx.verbose_line(format_args!("verbose: uv pid {}", child.id()));
    if let (Some(log), true) = (&log, ctx.verbose) {
        log.file_only(&format!("verbose: uv pid {}", child.id()));
    }
    if let Some(err) = assignment {
        ctx.verbose_line(format_args!(
            "verbose: could not assign uv {} to the job object: {err}",
            child.id()
        ));
    }
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut readers = Vec::new();
    let streams: [Option<Box<dyn std::io::Read + Send>>; 2] = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ];
    for stream in streams.into_iter().flatten() {
        let lines = Arc::clone(&lines);
        let log = log.clone();
        readers.push(thread::spawn(move || {
            let mut reader = BufReader::new(stream);
            while let LineEvent::Line(text) = read_limited_line(&mut reader, MAX_LINE_BYTES) {
                if let Some(log) = &log {
                    log.line("uv: ", true, &text);
                }
                if let Ok(mut all) = lines.lock()
                    && all.len() < 5000
                {
                    all.push(text);
                }
            }
        }));
    }
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if interrupt::is_set() {
                    ctx.job.terminate();
                    let _ = child.wait();
                    return Err(CliError::setup_interrupted());
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(err) => return Err(CliError::internal(format!("cannot wait for uv: {err}"))),
        }
    };
    for reader in readers {
        let _ = reader.join();
    }
    if interrupt::is_set() {
        // RULE-006: an interrupt that arrives with the exit wins over the step failure.
        return Err(CliError::setup_interrupted());
    }
    let lines = lines.lock().map(|all| all.clone()).unwrap_or_default();
    Ok(UvRun {
        success: status.success(),
        lines,
    })
}

impl UvRun {
    /// The last `error:` line uv printed, or the last line.
    pub fn last_error(&self) -> String {
        self.lines
            .iter()
            .rev()
            .find(|l| l.trim_start().starts_with("error:"))
            .or_else(|| self.lines.last())
            .map_or_else(
                || "uv failed without output".to_owned(),
                |l| l.trim().to_owned(),
            )
    }

    /// True when uv reported a hash mismatch.
    pub fn has_hash_mismatch(&self) -> bool {
        self.lines.iter().any(|l| l.contains("Hash mismatch"))
    }
}
