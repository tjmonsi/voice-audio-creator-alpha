//! Fake uv for the narrate tests (spec-narrate-test-strategy.md, Requirement: Test kit).
//!
//! Every invocation first appends its arguments, joined by single spaces, as one line to
//! `FAKE_UV_LOG` and one JSON line `{"args":[...],"env":{...}}` to `FAKE_UV_ENV_DUMP` (both when
//! set). Then:
//!
//! * `--version` prints `uv <FAKE_UV_VERSION, default 0.10.2> (test)` and exits 0.
//! * `python install ...`: with `--offline`, exits 0 only when `UV_PYTHON_CACHE_DIR` already
//!   holds a file with the name of `FAKE_UV_PY_ARCHIVE`, else 1. Without `--offline` it creates
//!   `UV_PYTHON_CACHE_DIR` and `UV_PYTHON_INSTALL_DIR`, copies the archive into the cache
//!   directory and exits 0.
//! * `sync ...`: sleeps `FAKE_UV_SYNC_SLEEP_MS` (default 0); when `FAKE_UV_SYNC_EXIT` is set,
//!   prints `FAKE_UV_SYNC_STDERR` to stderr and exits with that code, creating nothing; with
//!   `--offline` it exits 0 only when the venv marker files exist under
//!   `UV_PROJECT_ENVIRONMENT`, else 1; without `--offline` it creates the marker files (empty)
//!   and exits 0. `FAKE_UV_SYNC_NO_PACKAGE=1` leaves out the package `METADATA` file.
//! * Anything else exits `FAKE_UV_EXIT` (default 1).
//!
//! The behavior does not depend on the executable's name, so a copy named `uv.exe` works the
//! same.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Duration;

use serde_json::{Map, Value, json};

const MARKERS: [&[&str]; 3] = [
    &["pyvenv.cfg"],
    &["Scripts", "python.exe"],
    &["Lib", "site-packages", "chatterbox_tts-0.1.7.dist-info", "METADATA"],
];

/// A variable's value, or `None` when it is unset or empty.
fn var(name: &str) -> Option<String> {
    std::env::var_os(name)
        .map(|v| v.to_string_lossy().into_owned())
        .filter(|v| !v.is_empty())
}

fn append_line(path: &str, line: &str) {
    let file = fs::OpenOptions::new().create(true).append(true).open(path);
    match file {
        Ok(mut f) => {
            let mut text = String::with_capacity(line.len() + 1);
            text.push_str(line);
            text.push('\n');
            if let Err(e) = f.write_all(text.as_bytes()) {
                eprintln!("narrate-test-uv: cannot write {path}: {e}");
            }
        }
        Err(e) => eprintln!("narrate-test-uv: cannot open {path}: {e}"),
    }
}

fn record(args: &[String]) {
    if let Some(log) = var("FAKE_UV_LOG") {
        append_line(&log, &args.join(" "));
    }
    if let Some(dump) = var("FAKE_UV_ENV_DUMP") {
        let mut env = Map::new();
        for (k, v) in std::env::vars_os() {
            env.insert(
                k.to_string_lossy().into_owned(),
                Value::String(v.to_string_lossy().into_owned()),
            );
        }
        let entry = json!({ "args": args, "env": env });
        append_line(&dump, &entry.to_string());
    }
}

fn fail(code: i32, message: &str) -> ! {
    eprintln!("narrate-test-uv: {message}");
    exit(code);
}

fn marker_paths(venv: &Path) -> Vec<PathBuf> {
    MARKERS
        .iter()
        .map(|parts| parts.iter().fold(venv.to_path_buf(), |acc, p| acc.join(p)))
        .collect()
}

fn python_install(offline: bool) -> ! {
    let Some(archive) = var("FAKE_UV_PY_ARCHIVE") else {
        fail(1, "FAKE_UV_PY_ARCHIVE is not set");
    };
    let Some(cache) = var("UV_PYTHON_CACHE_DIR") else {
        fail(1, "UV_PYTHON_CACHE_DIR is not set");
    };
    let archive = PathBuf::from(archive);
    let Some(name) = archive.file_name() else {
        fail(1, "FAKE_UV_PY_ARCHIVE names no file");
    };
    let cached = Path::new(&cache).join(name);
    if offline {
        if cached.is_file() {
            exit(0);
        }
        fail(1, &format!("offline and {} is not cached", cached.display()));
    }
    let Some(install) = var("UV_PYTHON_INSTALL_DIR") else {
        fail(1, "UV_PYTHON_INSTALL_DIR is not set");
    };
    if let Err(e) = fs::create_dir_all(&cache).and_then(|_| fs::create_dir_all(&install)) {
        fail(1, &format!("cannot create the python directories: {e}"));
    }
    if let Err(e) = fs::copy(&archive, &cached) {
        fail(1, &format!("cannot copy {} to {}: {e}", archive.display(), cached.display()));
    }
    exit(0);
}

fn sync(offline: bool) -> ! {
    let sleep_ms = var("FAKE_UV_SYNC_SLEEP_MS").and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(0);
    if sleep_ms > 0 {
        std::thread::sleep(Duration::from_millis(sleep_ms));
    }
    if let Some(code) = var("FAKE_UV_SYNC_EXIT") {
        if let Some(text) = std::env::var_os("FAKE_UV_SYNC_STDERR") {
            eprintln!("{}", text.to_string_lossy());
        }
        exit(code.trim().parse::<i32>().unwrap_or(1));
    }
    let Some(venv) = var("UV_PROJECT_ENVIRONMENT") else {
        fail(1, "UV_PROJECT_ENVIRONMENT is not set");
    };
    let markers = marker_paths(Path::new(&venv));
    if offline {
        match markers.iter().find(|m| !m.is_file()) {
            None => exit(0),
            Some(missing) => fail(1, &format!("offline and {} is missing", missing.display())),
        }
    }
    let no_package = var("FAKE_UV_SYNC_NO_PACKAGE").as_deref() == Some("1");
    for marker in &markers {
        if no_package && marker.file_name().is_some_and(|n| n == "METADATA") {
            continue;
        }
        let made = marker
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| fs::File::create(marker).map(|_| ()));
        if let Err(e) = made {
            fail(1, &format!("cannot create {}: {e}", marker.display()));
        }
    }
    exit(0);
}

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    record(&args);
    let offline = args.iter().any(|a| a == "--offline");
    let first = args.first().map(String::as_str);
    if first.is_some_and(|f| f.starts_with('-')) && args.iter().any(|a| a == "--version") {
        println!("uv {} (test)", var("FAKE_UV_VERSION").unwrap_or_else(|| "0.10.2".to_string()));
        exit(0);
    }
    match first {
        Some("python") if args.get(1).map(String::as_str) == Some("install") => python_install(offline),
        Some("sync") => sync(offline),
        _ => {
            let code = var("FAKE_UV_EXIT").and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(1);
            eprintln!("narrate-test-uv: unsupported invocation: {}", args.join(" "));
            exit(code);
        }
    }
}
