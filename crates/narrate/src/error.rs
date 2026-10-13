//! The error catalog of spec-narrate-cli.md and the way errors are printed.

use std::fmt;
use std::path::Path;

use crate::output::say;

/// pinned uv download advice shared by `uv-not-found` and `uv-version` (spec-narrate-cli.md).
const UV_ZIP_FIX: &str = "download https://github.com/astral-sh/uv/releases/download/0.10.2/uv-x86_64-pc-windows-msvc.zip, check that its sha256 is 493ebbe0e06128d6ee4905e1ed5e2a433fb0f7cfc08b0eaca9fab4ca76778ae1, and put uv.exe on PATH";
const UV_SCRIPT_FIX: &str = "or run: powershell -ExecutionPolicy ByPass -c \"irm https://astral.sh/uv/0.10.2/install.ps1 | iex\"";
const REPAIR_ENV: &str = "run narrate setup to repair the environment";
const RESTORE_PINS: &str = "restore python/uv.lock and pins.json from the narrate release";

/// One entry of the error catalog, ready to print.
#[derive(Debug)]
pub struct CliError {
    /// Catalog code, the text inside `error[...]`.
    pub code: &'static str,
    /// Process exit code.
    pub exit: u8,
    /// Text after `error[<code>]: `.
    pub cause: String,
    /// One or more `  fix:` lines.
    pub fix: Vec<String>,
    /// `  detail:` lines, printed only with `--verbose`.
    pub details: Vec<String>,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}]: {}", self.code, self.cause)
    }
}

impl std::error::Error for CliError {}

impl CliError {
    fn new(code: &'static str, exit: u8, cause: impl Into<String>, fix: &str) -> CliError {
        CliError {
            code,
            exit,
            cause: cause.into(),
            fix: vec![fix.to_owned()],
            details: Vec::new(),
        }
    }

    /// Add `  detail:` lines (one per line of `text`).
    pub fn with_detail(mut self, text: impl AsRef<str>) -> CliError {
        self.details
            .extend(text.as_ref().lines().map(str::to_owned));
        self
    }

    /// Print the error, the fix lines, and with `verbose` the detail lines.
    pub fn report(&self, verbose: bool) {
        say(format_args!("error[{}]: {}", self.code, self.cause));
        for fix in &self.fix {
            say(format_args!("  fix: {fix}"));
        }
        if verbose {
            for detail in &self.details {
                say(format_args!("  detail: {detail}"));
            }
        }
    }

    // -- input and output problems (exit 3) ---------------------------------------------------

    pub fn input_extension(p: &Path) -> CliError {
        CliError::new(
            "input-extension",
            3,
            format!("unsupported script type: {}", p.display()),
            "use a .md, .markdown, or .txt file",
        )
    }

    pub fn input_not_found(p: &Path) -> CliError {
        CliError::new(
            "input-not-found",
            3,
            format!("input file not found: {}", p.display()),
            "check the path and try again",
        )
    }

    pub fn input_not_a_file(p: &Path) -> CliError {
        CliError::new(
            "input-not-a-file",
            3,
            format!("input path is not a file: {}", p.display()),
            "pass a script file, not a directory",
        )
    }

    pub fn input_unreadable(p: &Path, os: impl fmt::Display) -> CliError {
        CliError::new(
            "input-unreadable",
            3,
            format!("cannot read {}: {os}", p.display()),
            "check the file permissions",
        )
    }

    pub fn input_encoding(p: &Path, offset: usize) -> CliError {
        CliError::new(
            "input-encoding",
            3,
            format!(
                "{} is not valid UTF-8 (invalid byte at offset {offset})",
                p.display()
            ),
            "save the file with UTF-8 encoding",
        )
    }

    pub fn script_empty(p: &Path) -> CliError {
        CliError::new(
            "script-empty",
            3,
            format!("{} has no speakable text after conversion", p.display()),
            "add text outside code blocks and front matter",
        )
    }

    pub fn output_dir_missing(dir: &Path) -> CliError {
        CliError::new(
            "output-dir-missing",
            3,
            format!("output directory does not exist: {}", dir.display()),
            "create the directory or choose another -o path",
        )
    }

    pub fn output_is_directory(p: &Path) -> CliError {
        CliError::new(
            "output-is-directory",
            3,
            format!("output path is a directory: {}", p.display()),
            &format!("give a file name, for example -o {}\\out.wav", p.display()),
        )
    }

    pub fn output_is_input(p: &Path) -> CliError {
        CliError::new(
            "output-is-input",
            3,
            format!("the output would overwrite the input file: {}", p.display()),
            "choose a different -o path",
        )
    }

    pub fn output_not_writable(p: &Path, os: impl fmt::Display) -> CliError {
        CliError::new(
            "output-not-writable",
            3,
            format!("output path is not writable: {} ({os})", p.display()),
            "choose another directory, or close programs that use the file",
        )
    }

    pub fn voice_file_not_found(p: &Path) -> CliError {
        CliError::new(
            "voice-file-not-found",
            3,
            format!("voice file not found: {}", p.display()),
            "check the path to the WAV clip",
        )
    }

    pub fn voice_file_unreadable(p: &Path, detail: &str) -> CliError {
        CliError::new(
            "voice-file-unreadable",
            3,
            format!("{} is not a readable WAV file ({detail})", p.display()),
            "export the clip as a WAV file and try again",
        )
    }

    pub fn voice_file_unsupported(p: &Path, detail: &str) -> CliError {
        CliError::new(
            "voice-file-unsupported",
            3,
            format!(
                "{} uses an unsupported WAV encoding ({detail}); supported encodings: PCM 8, 16, 24, or 32-bit integer, or 32-bit float",
                p.display()
            ),
            "convert the clip to 16-bit PCM WAV",
        )
    }

    pub fn voice_file_too_short(p: &Path, seconds: f64) -> CliError {
        CliError::new(
            "voice-file-too-short",
            3,
            format!(
                "{} is {seconds:.1} s long; the voice clip must be longer than 5 s",
                p.display()
            ),
            "record or export a clip longer than 5 s (10 s or more is best)",
        )
    }

    // -- readiness (exit 4) -------------------------------------------------------------------

    pub fn not_set_up(list: &str) -> CliError {
        CliError::new(
            "not-set-up",
            4,
            format!("setup is incomplete: {list}"),
            "run narrate setup",
        )
    }

    pub fn pin_mismatch(dir: &Path, recorded: &str, binary: &str) -> CliError {
        CliError::new(
            "pin-mismatch",
            4,
            format!(
                "the data directory {} was set up with a different pin set (recorded {recorded}, this binary {binary})",
                dir.display()
            ),
            "run narrate setup",
        )
    }

    // -- generation runtime (exit 1) ----------------------------------------------------------

    pub fn worker_start(os: impl fmt::Display) -> CliError {
        CliError::new(
            "worker-start",
            1,
            format!("could not start the generation worker: {os}"),
            REPAIR_ENV,
        )
    }

    pub fn worker_protocol(detail: impl fmt::Display) -> CliError {
        CliError::new(
            "worker-protocol",
            1,
            format!("the generation worker sent an invalid message: {detail}"),
            REPAIR_ENV,
        )
    }

    pub fn worker_exited(code: Option<i32>, during: &str, log: &Path) -> CliError {
        let code = code.map_or_else(|| "unknown".to_owned(), |c| c.to_string());
        CliError::new(
            "worker-exited",
            1,
            format!("the generation worker exited unexpectedly (exit code {code}) {during}"),
            &format!("rerun with --verbose, or read {}", log.display()),
        )
    }

    pub fn cuda_unavailable() -> CliError {
        CliError::new(
            "cuda-unavailable",
            1,
            "CUDA is not available on this machine",
            "use --device auto or --device cpu, or check the NVIDIA driver",
        )
    }

    pub fn model_load(message: &str) -> CliError {
        CliError::new(
            "model-load",
            1,
            format!("could not load the Chatterbox-Turbo model: {message}"),
            "run narrate setup to repair the model files",
        )
    }

    pub fn watermark_unavailable() -> CliError {
        CliError::new(
            "watermark-unavailable",
            1,
            "the PerTh watermarker could not be loaded; narrate does not write unwatermarked audio",
            REPAIR_ENV,
        )
    }

    pub fn voice_prepare(voice: &str, message: &str) -> CliError {
        CliError::new(
            "voice-prepare",
            1,
            format!("could not prepare the {voice} voice: {message}"),
            "use another clip or run narrate setup",
        )
    }

    pub fn chunk_failed(i: usize, n: usize, message: &str) -> CliError {
        CliError::new(
            "chunk-failed",
            1,
            format!("generation failed on chunk {i} of {n}: {message}"),
            "rerun with --verbose for details",
        )
    }

    pub fn gpu_out_of_memory(i: usize, n: usize) -> CliError {
        CliError::new(
            "gpu-out-of-memory",
            1,
            format!("the GPU ran out of memory on chunk {i} of {n}"),
            "close other programs that use the GPU, or use --device cpu",
        )
    }

    pub fn gpu_out_of_memory_loading() -> CliError {
        CliError::new(
            "gpu-out-of-memory",
            1,
            "the GPU ran out of memory while loading the model",
            "close other programs that use the GPU, or use --device cpu",
        )
    }

    pub fn wav_too_large(limit: u64) -> CliError {
        CliError::new(
            "wav-too-large",
            1,
            format!("the audio would exceed the WAV size limit of {limit} bytes"),
            "split the script into smaller files",
        )
    }

    pub fn output_write(p: &Path, os: impl fmt::Display) -> CliError {
        CliError::new(
            "output-write",
            1,
            format!("could not write {}: {os}", p.display()),
            "check free disk space and try again",
        )
    }

    pub fn interrupted() -> CliError {
        CliError::new(
            "interrupted",
            130,
            "interrupted; no output was written",
            "run the command again",
        )
    }

    // -- setup (exit 5) -----------------------------------------------------------------------

    fn uv_advice(code: &'static str, cause: String) -> CliError {
        CliError {
            code,
            exit: 5,
            cause,
            fix: vec![UV_ZIP_FIX.to_owned(), UV_SCRIPT_FIX.to_owned()],
            details: Vec::new(),
        }
    }

    pub fn uv_not_found(missing_path: Option<&Path>) -> CliError {
        let cause = match missing_path {
            Some(p) => format!("uv was not found at {}", p.display()),
            None => "uv was not found on PATH".to_owned(),
        };
        CliError::uv_advice("uv-not-found", cause)
    }

    pub fn uv_version(found: &str) -> CliError {
        CliError::uv_advice(
            "uv-version",
            format!("uv {found} is installed, but narrate needs uv 0.10.2"),
        )
    }

    pub fn source_not_allowed(cause: impl Into<String>) -> CliError {
        CliError::new("source-not-allowed", 5, cause, RESTORE_PINS)
    }

    pub fn pin_invalid(cause: impl Into<String>) -> CliError {
        CliError::new("pin-invalid", 5, cause, RESTORE_PINS)
    }

    pub fn hash_mismatch(artifact: &str, expected: &str, actual: &str) -> CliError {
        CliError::new(
            "hash-mismatch",
            5,
            format!(
                "{artifact} does not match its pinned sha256 (expected {expected}, got {actual}); the file was discarded"
            ),
            "run narrate setup again; if this repeats, do not use this network",
        )
    }

    pub fn setup_step_failed(step: &str, message: impl fmt::Display) -> CliError {
        CliError::new(
            "setup-step-failed",
            5,
            format!("setup failed at step {step}: {message}"),
            "check the network connection and run narrate setup again; completed steps are kept",
        )
    }

    pub fn setup_busy(dir: &Path) -> CliError {
        CliError::new(
            "setup-busy",
            5,
            format!("another narrate setup is running for {}", dir.display()),
            "wait for it to finish, then run narrate setup again",
        )
    }

    pub fn data_dir_unwritable(dir: &Path, os: impl fmt::Display) -> CliError {
        CliError::new(
            "data-dir-unwritable",
            5,
            format!("cannot write to the data directory {}: {os}", dir.display()),
            "set NARRATE_DATA_DIR to a writable directory",
        )
    }

    pub fn setup_interrupted() -> CliError {
        CliError::new(
            "setup-interrupted",
            130,
            "setup was interrupted",
            "run narrate setup again to continue",
        )
    }

    // -- other --------------------------------------------------------------------------------

    pub fn invalid_test_hook(variable: &str, value: &str) -> CliError {
        CliError::new(
            "invalid-test-hook",
            2,
            format!("{variable} has an invalid value: {value}"),
            "unset it or give a valid value",
        )
    }

    pub fn internal(message: impl Into<String>) -> CliError {
        CliError::new(
            "internal",
            1,
            message,
            "rerun with --verbose and report the output",
        )
    }
}

impl From<anyhow::Error> for CliError {
    fn from(err: anyhow::Error) -> CliError {
        let mut chain = err.chain().map(ToString::to_string);
        let head = chain.next().unwrap_or_else(|| "unknown error".to_owned());
        let mut error = CliError::internal(head);
        error.details.extend(chain);
        error
    }
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> CliError {
        CliError::internal(err.to_string())
    }
}
