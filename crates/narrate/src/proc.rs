//! Child processes: environment rules, process groups, and the kill-on-close job object
//! (spec-narrate-setup.md, Child process environment and Process creation).

use std::ffi::{OsStr, OsString, c_void};
use std::io::{self, BufRead, BufReader, Read};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};

use crate::hooks::{self, WORKER_HOOKS};

const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Variable-name prefixes that never reach a child (compared case-insensitively).
const REMOVED_PREFIXES: [&str; 9] = [
    "UV_",
    "HF_",
    "HUGGINGFACE_",
    "TRANSFORMERS_",
    "PIP_",
    "PYTHON",
    "CONDA_",
    "VIRTUAL_ENV",
    "NARRATE_TEST_",
];

/// A Windows job object that kills every process in it when the last handle closes, so a killed
/// `narrate` cannot leave a worker holding VRAM.
pub struct Job {
    handle: isize,
}

impl Job {
    pub fn new() -> io::Result<Job> {
        // SAFETY: both pointer arguments may be null (no security attributes, unnamed job).
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the structure is plain data for which all-zero is a valid initial value.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `handle` is a valid job handle and `info` outlives the call; the size matches the class.
        let ok = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast::<c_void>(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            let err = io::Error::last_os_error();
            // SAFETY: `handle` was returned by CreateJobObjectW above and is closed once.
            unsafe { CloseHandle(handle) };
            return Err(err);
        }
        Ok(Job {
            handle: handle as isize,
        })
    }

    /// Put `child` (and the processes it starts later) in the job.
    pub fn assign(&self, child: &Child) -> io::Result<()> {
        // SAFETY: both handles are valid for the duration of the call.
        let ok = unsafe { AssignProcessToJobObject(self.handle as _, child.as_raw_handle() as _) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Terminate every process in the job.
    pub fn terminate(&self) {
        // SAFETY: the handle is valid; failure only means the job is already empty.
        unsafe { TerminateJobObject(self.handle as _, 1) };
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateJobObjectW and is closed exactly once; closing the last
        // handle kills any process still in the job.
        unsafe { CloseHandle(self.handle as _) };
    }
}

/// What to start and with which environment.
pub struct Spawn<'a> {
    pub program: &'a Path,
    pub args: Vec<OsString>,
    pub cwd: Option<&'a Path>,
    pub env: Vec<(OsString, OsString)>,
    /// Pipe stdin (workers) instead of connecting it to nothing (uv).
    pub pipe_stdin: bool,
}

/// Start a child in its own process group without a console and put it in `job`.
///
/// A failed job assignment is returned as the second value so the caller can note it; the stdin
/// watchdog in the worker still ends it when `narrate` goes away.
pub fn spawn(job: &Job, spec: &Spawn<'_>) -> io::Result<(Child, Option<io::Error>)> {
    let mut command = Command::new(spec.program);
    command
        .args(&spec.args)
        .env_clear()
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(if spec.pipe_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    if let Some(cwd) = spec.cwd {
        command.current_dir(cwd);
    }
    let child = command.spawn()?;
    let assignment = job.assign(&child).err();
    Ok((child, assignment))
}

/// The parent environment with the variables of [`REMOVED_PREFIXES`] taken out.
pub fn base_env() -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(name, _)| !is_removed(name))
        .collect()
}

fn is_removed(name: &OsStr) -> bool {
    let upper = name.to_string_lossy().to_ascii_uppercase();
    REMOVED_PREFIXES
        .iter()
        .any(|prefix| upper.starts_with(prefix))
}

/// Hook variables a worker keeps in test builds when the parent set them.
pub fn worker_hook_env() -> Vec<(OsString, OsString)> {
    if !hooks::ENABLED {
        return Vec::new();
    }
    WORKER_HOOKS
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (OsString::from(name), value)))
        .collect()
}

/// Set `name` to `value`, replacing any existing entry (names compare case-insensitively).
pub fn set_env(env: &mut Vec<(OsString, OsString)>, name: &str, value: impl Into<OsString>) {
    env.retain(|(existing, _)| !existing.to_string_lossy().eq_ignore_ascii_case(name));
    env.push((OsString::from(name), value.into()));
}

/// Upper bound for one protocol line (spec-narrate-worker.md, Performance considerations).
pub const MAX_LINE_BYTES: usize = 32 * 1024 * 1024;

/// What a line reader produced.
pub enum LineEvent {
    Line(String),
    TooLong,
    Eof,
    Failed,
}

/// Read the next line of at most `limit` bytes, stripping the line break.
pub fn read_limited_line<R: Read>(reader: &mut BufReader<R>, limit: usize) -> LineEvent {
    let mut line: Vec<u8> = Vec::new();
    loop {
        let available = match reader.fill_buf() {
            Ok(buf) => buf,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return LineEvent::Failed,
        };
        if available.is_empty() {
            return if line.is_empty() {
                LineEvent::Eof
            } else {
                LineEvent::Line(finish(line))
            };
        }
        let (used, done) = match available.iter().position(|&b| b == b'\n') {
            Some(at) => (at + 1, true),
            None => (available.len(), false),
        };
        line.extend_from_slice(&available[..used]);
        reader.consume(used);
        if line.len() > limit {
            return LineEvent::TooLong;
        }
        if done {
            return LineEvent::Line(finish(line));
        }
    }
}

fn finish(mut line: Vec<u8>) -> String {
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    String::from_utf8_lossy(&line).into_owned()
}

/// Wait for `child` to exit for up to `grace`; true when it exited.
pub fn wait_for_exit(child: &mut Child, grace: Duration) -> bool {
    let deadline = Instant::now() + grace;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) | Err(_) => return false,
        }
    }
}
