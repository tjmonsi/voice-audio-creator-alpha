//! `narrate`: turn a Markdown or text script into a narrated WAV file.
//!
//! The binary owns everything observable (command line, text conversion, chunking, WAV output,
//! readiness checks, error messages). A Python worker, started from a private environment that
//! `narrate setup` builds, owns only model work.

mod cli;
mod codec;
mod ctx;
mod datadir;
mod error;
mod fsutil;
mod generate;
mod hashing;
mod hooks;
mod interrupt;
mod lockscan;
mod logsink;
mod output;
mod pins;
mod proc;
mod ready;
mod setup;
mod status;
mod uv;
mod worker;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::ExitCode;
use std::sync::Mutex;

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};

use cli::{Cli, Command};
use ctx::Ctx;
use error::CliError;
use hooks::Hooks;

/// Text of the most recent panic, for the `internal` error report.
static PANIC_TEXT: Mutex<String> = Mutex::new(String::new());

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) if cli.command.is_some() && cli.generate.any_given() => {
            let message = "generation options cannot be used with the setup and status commands";
            return usage_exit(&Cli::command().error(ErrorKind::ArgumentConflict, message));
        }
        Ok(cli) => cli,
        Err(err) => return usage_exit(&err),
    };
    let verbose = cli.verbose;

    // [FR-005] A defect in one command must end as `error[internal]`, never as a raw panic dump.
    std::panic::set_hook(Box::new(|info| {
        if let Ok(mut text) = PANIC_TEXT.lock() {
            *text = info.to_string();
        }
    }));
    let outcome = catch_unwind(AssertUnwindSafe(|| run(cli)));
    let code = match outcome {
        Ok(Ok(code)) => code,
        Ok(Err(error)) => {
            error.report(verbose);
            error.exit
        }
        Err(_) => {
            let message = PANIC_TEXT.lock().map(|t| t.clone()).unwrap_or_default();
            let error = CliError::internal("narrate stopped on an unexpected internal error")
                .with_detail(message);
            error.report(verbose);
            error.exit
        }
    };
    ExitCode::from(code)
}

/// Print a clap error (help and version go to stdout) and pick the exit code. Usage errors always
/// show the usage, which clap leaves out when it prints a suggestion (spec-narrate-cli.md).
fn usage_exit(err: &clap::Error) -> ExitCode {
    let _ = err.print();
    if !err.use_stderr() {
        return ExitCode::SUCCESS;
    }
    if !err.to_string().contains("Usage:") {
        let mut command = Cli::command();
        output::say(format_args!("{}", command.render_usage()));
    }
    ExitCode::from(2)
}

fn run(cli: Cli) -> Result<u8, CliError> {
    let hooks = Hooks::from_env()?;
    let ctx = Ctx::new(cli.verbose, hooks)?;
    if let Err(err) = interrupt::install() {
        ctx.verbose_line(format_args!(
            "verbose: cannot install the Ctrl+C handler: {err}"
        ));
    }
    match cli.command {
        Some(Command::Setup) => setup::run(&ctx).map(|()| 0),
        Some(Command::Status) => status::run(&ctx),
        None => {
            let options = cli
                .generate
                .into_options()
                .ok_or_else(|| CliError::internal("no script file was given"))?;
            generate::run(&ctx, &options).map(|()| 0)
        }
    }
}
