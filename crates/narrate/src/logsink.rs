//! Where child output lines go: a log file, and with `--verbose` stderr as well.

use std::fs::File;
use std::io::Write;
use std::sync::Mutex;

use crate::output::say;

/// Shared by the reader threads of one child.
pub struct LogSink {
    file: Option<Mutex<File>>,
    verbose: bool,
}

impl LogSink {
    pub fn new(file: Option<File>, verbose: bool) -> LogSink {
        LogSink {
            file: file.map(Mutex::new),
            verbose,
        }
    }

    /// Record `text`; `prefix` (for example `uv: `) is shown with `--verbose` and written to the file
    /// only when `prefix_in_file` is set.
    pub fn line(&self, prefix: &str, prefix_in_file: bool, text: &str) {
        if let Some(file) = &self.file
            && let Ok(mut file) = file.lock()
        {
            let _ = if prefix_in_file {
                writeln!(file, "{prefix}{text}")
            } else {
                writeln!(file, "{text}")
            };
        }
        if self.verbose {
            say(format_args!("{prefix}{text}"));
        }
    }

    /// Record a `verbose:` line in the file without printing it again (the caller printed it).
    pub fn file_only(&self, text: &str) {
        if let Some(file) = &self.file
            && let Ok(mut file) = file.lock()
        {
            let _ = writeln!(file, "{text}");
        }
    }
}
