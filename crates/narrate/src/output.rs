//! Stream discipline (spec-narrate-cli.md): results go to stdout, every other line to stderr.
//! Writes ignore failures so a closed pipe cannot turn into a panic.

use std::fmt::Arguments;
use std::io::Write;

/// Write one line to stderr.
pub fn say(args: Arguments<'_>) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "{args}");
}

/// Write one result line to stdout.
pub fn result_line(args: Arguments<'_>) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{args}");
    let _ = out.flush();
}
