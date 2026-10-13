//! `narrate status` (spec-narrate-setup.md, Requirement 8).

use std::time::Duration;

use serde_json::{Value, json};

use crate::ctx::Ctx;
use crate::error::CliError;
use crate::fsutil::RunTemp;
use crate::hooks::VERSION_LINE;
use crate::output::result_line;
use crate::pins::PinSet;
use crate::ready::{PACKAGE_VERSION, Package, Readiness, Record};
use crate::worker::{Mode, StderrTarget, WorkerError, WorkerSession};

/// Print the status report; returns the exit code (0 ready, 4 not ready).
pub fn run(ctx: &Ctx) -> Result<u8, CliError> {
    let pins = PinSet::load(&ctx.hooks)?;
    let ready = Readiness::check(&ctx.data, &pins);
    ctx.verbose_line(format_args!(
        "verbose: data directory {}",
        ctx.data.root().display()
    ));

    let cuda = if ready.environment {
        probe_cuda(ctx)?
    } else {
        "unknown (environment missing)".to_owned()
    };

    let list = |missing: &[String]| {
        if missing.is_empty() {
            "present".to_owned()
        } else {
            format!("missing ({})", missing.join(", "))
        }
    };
    let present = |ok: bool| if ok { "present" } else { "missing" };
    result_line(format_args!("{VERSION_LINE}"));
    result_line(format_args!(
        "data directory: {}",
        ctx.data.root().display()
    ));
    result_line(format_args!("environment: {}", present(ready.environment)));
    let package = match &ready.package {
        Package::Present => format!("present (chatterbox-tts {PACKAGE_VERSION})"),
        Package::Missing => "missing".to_owned(),
        Package::Wrong(found) => {
            format!("wrong version (chatterbox-tts {found}, expected {PACKAGE_VERSION})")
        }
    };
    result_line(format_args!("package: {package}"));
    result_line(format_args!(
        "turbo weights: {}",
        list(&ready.turbo_missing)
    ));
    result_line(format_args!(
        "kokoro weights: {}",
        list(&ready.kokoro_missing)
    ));
    result_line(format_args!("female clip: {}", present(ready.female)));
    result_line(format_args!("male clip: {}", present(ready.male)));
    result_line(format_args!("builtin clip: {}", present(ready.builtin)));
    let pin = match &ready.record {
        Record::Missing => format!("missing (binary {}, recorded none)", ready.binary_id),
        Record::Recorded(id) if *id == ready.binary_id => {
            format!("match (binary {}, recorded {id})", ready.binary_id)
        }
        Record::Recorded(id) => format!("mismatch (binary {}, recorded {id})", ready.binary_id),
    };
    result_line(format_args!("pin set: {pin}"));
    result_line(format_args!("cuda: {cuda}"));
    if ready.is_ready() {
        result_line(format_args!("status: ready"));
        Ok(0)
    } else {
        result_line(format_args!("status: not ready (run narrate setup)"));
        Ok(4)
    }
}

/// Ask a short-lived worker about CUDA; any failure reads `unknown (probe failed)`.
fn probe_cuda(ctx: &Ctx) -> Result<String, CliError> {
    let failed = || "unknown (probe failed)".to_owned();
    let Ok(run_tmp) = RunTemp::create(&ctx.data) else {
        return Ok(failed());
    };
    let mut session =
        match WorkerSession::start(ctx, run_tmp.path(), Mode::Offline, StderrTarget::Discard) {
            Ok(session) => session,
            Err(WorkerError::Interrupted) => return Err(CliError::interrupted()),
            Err(_) => return Ok(failed()),
        };
    let reply = session.request("probe", json!({}), Some(Duration::from_secs(120)));
    let text = match reply {
        Ok(value) => describe_probe(&value),
        Err(WorkerError::Interrupted) => {
            session.abort(Duration::from_secs(3));
            return Err(CliError::interrupted());
        }
        Err(_) => None,
    };
    session.shutdown();
    Ok(text.unwrap_or_else(failed))
}

fn describe_probe(reply: &Value) -> Option<String> {
    let torch = reply.get("torch")?.as_str()?;
    if reply.get("cuda_available")?.as_bool()? {
        let name = reply
            .get("device_name")
            .and_then(Value::as_str)
            .unwrap_or("unknown device");
        Some(format!("available ({name}, torch {torch})"))
    } else {
        Some(format!("not available (torch {torch})"))
    }
}
