//! Console signal and process helper for the narrate tests and GPU QA.
//!
//! * `narrate-test-signal <process-group-id>` sends `CTRL_BREAK_EVENT` to that process group
//!   (exit 0 on success, 1 on failure). The group must have been created with
//!   `CREATE_NEW_PROCESS_GROUP` and must share this process's console.
//! * `narrate-test-signal alive <pid>` exits 0 while the process exists and has not exited
//!   (`PROCESS_QUERY_LIMITED_INFORMATION`, exit code still `STILL_ACTIVE`), else 1. A process
//!   that exists but cannot be opened (access denied) counts as alive.
//! * `narrate-test-signal spawn-group <exe> [args...]` starts the program with
//!   `CREATE_NEW_PROCESS_GROUP` and inherited stdio, prints its pid as the first stdout line,
//!   waits for it and exits with its exit code (125 when the program cannot be started). It
//!   exists because PowerShell `Start-Process` cannot set that flag: QA scripts and instructions
//!   start `narrate` through it and then send `narrate-test-signal <pid>`.
//!
//! The pinned `windows-sys` types `BOOL` and `HANDLE` are never named; results are compared with
//! `!= 0` and `is_null()` so type inference binds them.

use std::io::Write;
use std::os::windows::process::CommandExt;
use std::process::{Command, exit};

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const STILL_ACTIVE: u32 = 259;
const ERROR_ACCESS_DENIED: i32 = 5;

fn usage() -> ! {
    eprintln!(
        "usage: narrate-test-signal <process-group-id>\n       narrate-test-signal alive <pid>\n       narrate-test-signal spawn-group <exe> [args...]"
    );
    exit(2);
}

fn send_break(group: u32) -> bool {
    // SAFETY: plain Win32 call with value arguments.
    let ok = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, group) } != 0;
    if !ok {
        eprintln!(
            "narrate-test-signal: GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, {group}) failed: {}",
            std::io::Error::last_os_error()
        );
    }
    ok
}

fn alive(pid: u32) -> bool {
    // SAFETY: OpenProcess, GetExitCodeProcess and CloseHandle are called with a handle that this
    // function opened and closes exactly once.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return std::io::Error::last_os_error().raw_os_error() == Some(ERROR_ACCESS_DENIED);
        }
        let mut code: u32 = 0;
        let got = GetExitCodeProcess(handle, &mut code) != 0;
        CloseHandle(handle);
        got && code == STILL_ACTIVE
    }
}

fn spawn_group(args: &[String]) -> ! {
    let Some(exe) = args.first() else { usage() };
    let mut cmd = Command::new(exe);
    cmd.args(&args[1..]).creation_flags(CREATE_NEW_PROCESS_GROUP);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("narrate-test-signal: cannot start {exe}: {e}");
            exit(125);
        }
    };
    {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{}", child.id());
        let _ = out.flush();
    }
    match child.wait() {
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("narrate-test-signal: cannot wait for {exe}: {e}");
            exit(125);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => usage(),
        Some("alive") => {
            let Some(pid) = args.get(1).and_then(|p| p.parse::<u32>().ok()) else { usage() };
            exit(if alive(pid) { 0 } else { 1 });
        }
        Some("spawn-group") => spawn_group(&args[1..]),
        Some(id) => {
            let Ok(group) = id.parse::<u32>() else { usage() };
            exit(if send_break(group) { 0 } else { 1 });
        }
    }
}
