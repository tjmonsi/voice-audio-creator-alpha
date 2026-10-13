//! State shared by every command of one run.

use std::fmt::Arguments;
use std::sync::Arc;

use crate::datadir::DataDir;
use crate::error::CliError;
use crate::hooks::Hooks;
use crate::output::say;
use crate::proc::Job;

pub struct Ctx {
    pub verbose: bool,
    pub hooks: Hooks,
    pub data: DataDir,
    /// Kill-on-close job object that holds every child of this run.
    pub job: Arc<Job>,
}

impl Ctx {
    pub fn new(verbose: bool, hooks: Hooks) -> Result<Ctx, CliError> {
        let data = DataDir::resolve()?;
        let job = Job::new()
            .map_err(|err| CliError::internal(format!("cannot create the job object: {err}")))?;
        Ok(Ctx {
            verbose,
            hooks,
            data,
            job: Arc::new(job),
        })
    }

    /// Print a line to stderr only with `--verbose`.
    pub fn verbose_line(&self, args: Arguments<'_>) {
        if self.verbose {
            say(args);
        }
    }
}
