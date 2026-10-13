//! Test hooks (spec-narrate-cli.md, Requirement: Test hooks honored only in test builds).
//!
//! Release builds ignore every hook variable, so the shipped binary cannot be redirected to another
//! worker, uv, or lock file through the environment.

use std::path::PathBuf;

use crate::error::CliError;

/// True in builds made with `--features test-hooks`.
pub const ENABLED: bool = cfg!(feature = "test-hooks");

/// The line printed for `--version` and the first line of `status`.
pub const VERSION_LINE: &str = if ENABLED {
    concat!(
        "narrate ",
        env!("CARGO_PKG_VERSION"),
        " (test hooks enabled)"
    )
} else {
    concat!("narrate ", env!("CARGO_PKG_VERSION"))
};

/// Hook variables the worker reads itself; `narrate` validates them and passes them through.
pub const WORKER_HOOKS: [&str; 3] = [
    "NARRATE_TEST_FAIL_CHUNK",
    "NARRATE_TEST_CRASH_CHUNK",
    "NARRATE_TEST_WATERMARK_FAIL",
];

/// Hook values that `narrate` itself acts on.
#[derive(Debug, Default, Clone)]
pub struct Hooks {
    /// `NARRATE_WORKER_CMD`: executable that replaces the Python worker.
    pub worker_cmd: Option<PathBuf>,
    /// `NARRATE_UV`: executable that replaces `uv.exe` from PATH.
    pub uv: Option<PathBuf>,
    /// `NARRATE_TEST_WAV_LIMIT_BYTES`: replaces the 4 GiB WAV limit.
    pub wav_limit: Option<u64>,
    /// `NARRATE_TEST_PINS_FILE`: replaces the embedded `pins.json`.
    pub pins_file: Option<PathBuf>,
    /// `NARRATE_TEST_LOCK_FILE`: replaces the embedded `uv.lock`.
    pub lock_file: Option<PathBuf>,
}

impl Hooks {
    /// Read and validate the hook variables; an empty set in release builds.
    pub fn from_env() -> Result<Hooks, CliError> {
        if !ENABLED {
            return Ok(Hooks::default());
        }
        for name in ["NARRATE_TEST_FAIL_CHUNK", "NARRATE_TEST_CRASH_CHUNK"] {
            if let Some(value) = env_string(name) {
                match value.parse::<u64>() {
                    Ok(n) if n >= 1 => {}
                    _ => return Err(CliError::invalid_test_hook(name, &value)),
                }
            }
        }
        if let Some(value) = env_string("NARRATE_TEST_WATERMARK_FAIL")
            && value != "1"
        {
            return Err(CliError::invalid_test_hook(
                "NARRATE_TEST_WATERMARK_FAIL",
                &value,
            ));
        }
        let wav_limit = match env_string("NARRATE_TEST_WAV_LIMIT_BYTES") {
            Some(value) => match value.parse::<u64>() {
                Ok(n) if n >= 44 => Some(n),
                _ => {
                    return Err(CliError::invalid_test_hook(
                        "NARRATE_TEST_WAV_LIMIT_BYTES",
                        &value,
                    ));
                }
            },
            None => None,
        };
        Ok(Hooks {
            worker_cmd: env_path("NARRATE_WORKER_CMD"),
            uv: env_path("NARRATE_UV"),
            wav_limit,
            pins_file: env_path("NARRATE_TEST_PINS_FILE"),
            lock_file: env_path("NARRATE_TEST_LOCK_FILE"),
        })
    }
}

fn env_string(name: &str) -> Option<String> {
    std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}
