//! `narrate setup` (spec-narrate-setup.md, Requirements 1 to 6 and 9).
//!
//! Six steps, each skipping work that is already complete and verified: `preflight`, `python`,
//! `environment`, `models`, `voices`, `record`. `setup.json` is written last.

use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use narrate_core::{SAMPLE_RATE, f32_to_pcm16, inspect_clip, write_wav_pcm16};
use serde_json::{Value, json};

use crate::codec::decode_f32_hex;
use crate::ctx::Ctx;
use crate::datadir::DataDir;
use crate::error::CliError;
use crate::fsutil::{
    RunTemp, clean_stale_staging_clips, clean_stale_tmp, unix_seconds, write_atomic,
    write_atomic_if_changed,
};
use crate::hashing::sha256_file;
use crate::interrupt;
use crate::logsink::LogSink;
use crate::output::say;
use crate::pins::{KOKORO_DIR, PYPROJECT, PinSet, TURBO_DIR};
use crate::ready::{Package, Readiness, Record, read_record};
use crate::uv::{self, UV_VERSION, UvEnv, UvRun};
use crate::worker::{Mode, StderrTarget, WorkerError, WorkerSession, traceback_details};

const STEPS: usize = 6;
const CLIPS: [&str; 3] = ["female", "male", "builtin"];

/// Run every setup step; prints progress to stderr and nothing to stdout.
pub fn run(ctx: &Ctx) -> Result<(), CliError> {
    say(format_args!(
        "setup: data directory {}",
        ctx.data.root().display()
    ));

    // Step 1: preflight. Nothing is created until uv, the pins, and the lock all pass.
    let uv_path = uv::locate(ctx)?;
    let found = uv::version(ctx, &uv_path)?;
    if found != UV_VERSION {
        return Err(CliError::uv_version(&found));
    }
    let pins = PinSet::load(&ctx.hooks)?;
    pins.validate()?;

    let data = &ctx.data;
    fs::create_dir_all(data.root())
        .map_err(|err| CliError::data_dir_unwritable(data.root(), err))?;
    let _lock = take_lock(data)?;
    fs::create_dir_all(data.logs())
        .map_err(|err| CliError::data_dir_unwritable(data.root(), err))?;
    let log_file = File::create(data.logs().join("setup-last.log"))
        .map_err(|err| CliError::data_dir_unwritable(data.root(), err))?;
    let log = Arc::new(LogSink::new(Some(log_file), ctx.verbose));
    let run_tmp =
        RunTemp::create(data).map_err(|err| CliError::data_dir_unwritable(data.root(), err))?;
    write_project_files(data, &pins)
        .map_err(|err| CliError::data_dir_unwritable(data.root(), err))?;
    clean_stale_staging_clips(data);
    clean_stale_tmp(data);
    progress(1, "preflight", &format!("ok (uv {found})"));

    let uv_env = UvEnv {
        ctx,
        uv: &uv_path,
        run_tmp: run_tmp.path(),
        log: Arc::clone(&log),
    };
    let python = step_python(&uv_env, &pins)?;
    progress(2, "python", &python);
    let environment = step_environment(&uv_env, &pins)?;
    progress(3, "environment", &environment);
    let models = step_models(ctx, run_tmp.path(), &log, &pins)?;
    progress(4, "models", &models);
    let voices = step_voices(ctx, run_tmp.path(), &log, &pins)?;
    progress(5, "voices", &voices);
    step_record(data, &pins)?;
    progress(6, "record", &format!("ok (pin set {})", &pins.id[..12]));
    say(format_args!("setup: ready"));
    Ok(())
}

fn progress(step: usize, name: &str, result: &str) {
    say(format_args!("setup: [{step}/{STEPS}] {name}: {result}"));
}

/// Hold `setup.lock` exclusively for the life of the run.
fn take_lock(data: &DataDir) -> Result<File, CliError> {
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data.setup_lock())
        .map_err(|err| CliError::data_dir_unwritable(data.root(), err))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(CliError::setup_busy(data.root())),
        Err(TryLockError::Error(err)) => Err(CliError::data_dir_unwritable(data.root(), err)),
    }
}

/// Copies of the pin set that make the data directory self-describing (FR-017).
fn write_project_files(data: &DataDir, pins: &PinSet) -> std::io::Result<()> {
    let project = data.project();
    write_atomic_if_changed(&project.join("pyproject.toml"), PYPROJECT.as_bytes())?;
    write_atomic_if_changed(&project.join("uv.lock"), &pins.lock_bytes)?;
    write_atomic_if_changed(&project.join("pins.json"), &pins.pins_bytes)
}

fn check_interrupt() -> Result<(), CliError> {
    if interrupt::is_set() {
        Err(CliError::setup_interrupted())
    } else {
        Ok(())
    }
}

// -- step 2: python ---------------------------------------------------------------------------

fn step_python(uv: &UvEnv<'_>, pins: &PinSet) -> Result<String, CliError> {
    let data = &uv.ctx.data;
    let version = pins.pins.python.version.as_str();
    let install = |offline: bool| -> Result<UvRun, CliError> {
        let mut args = vec!["python", "install", version, "--no-bin", "--no-registry"];
        if offline {
            args.push("--offline");
        }
        uv.run(&args, false)
    };

    let offline = install(true)?;
    if offline.success
        && cached_archive_hash(&data.python_cache())
            .is_some_and(|hashes| hashes.contains(&pins.pins.python.sha256))
    {
        return Ok("ok (no network used)".to_owned());
    }
    // Only this program's own directories are removed (RULE-004).
    let _ = fs::remove_dir_all(data.python_dir());
    let _ = fs::remove_dir_all(data.python_cache());
    let online = install(false)?;
    if !online.success {
        return Err(CliError::setup_step_failed("python", online.last_error())
            .with_detail(online.lines.join("\n")));
    }
    let hashes = cached_archive_hash(&data.python_cache()).unwrap_or_default();
    if hashes.contains(&pins.pins.python.sha256) {
        return Ok("ok (downloaded)".to_owned());
    }
    let _ = fs::remove_dir_all(data.python_dir());
    let _ = fs::remove_dir_all(data.python_cache());
    let actual = hashes.first().map_or("no archive found", String::as_str);
    Err(CliError::hash_mismatch(
        &pins.pins.python.url,
        &pins.pins.python.sha256,
        actual,
    ))
}

/// sha256 of every file below the Python archive cache.
fn cached_archive_hash(dir: &Path) -> Option<Vec<String>> {
    fn walk(dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if let Ok(hash) = sha256_file(&path) {
                out.push(hash);
            }
        }
    }
    dir.is_dir().then(|| {
        let mut hashes = Vec::new();
        walk(dir, &mut hashes);
        hashes
    })
}

// -- step 3: environment ----------------------------------------------------------------------

fn step_environment(uv: &UvEnv<'_>, pins: &PinSet) -> Result<String, CliError> {
    let data = &uv.ctx.data;
    let project = data.project();
    let project = project.to_string_lossy();
    let version = pins.pins.python.version.as_str();
    let sync = |offline: bool| -> Result<UvRun, CliError> {
        let mut args = vec![
            "sync",
            "--project",
            project.as_ref(),
            "--frozen",
            "--managed-python",
            "--python",
            version,
            "--no-progress",
        ];
        if offline {
            args.push("--offline");
        }
        uv.run(&args, true)
    };

    let mut result = "ok (no network used)";
    let mut run = sync(true)?;
    if !run.success {
        result = "ok (downloaded)";
        run = sync(false)?;
    }
    if !run.success {
        return Err(sync_failure(&run));
    }
    let ready = Readiness::check(data, pins);
    if ready.package != Package::Present || !ready.environment {
        return Err(CliError::setup_step_failed(
            "environment",
            "the chatterbox-tts 0.1.7 package is missing after uv sync",
        ));
    }
    Ok(result.to_owned())
}

/// `hash-mismatch` when uv says so, else `setup-step-failed` with uv's last error line.
fn sync_failure(run: &UvRun) -> CliError {
    if run.has_hash_mismatch() {
        let (artifact, expected, computed) = parse_uv_mismatch(&run.lines);
        return CliError::hash_mismatch(&artifact, &expected, &computed)
            .with_detail(run.lines.join("\n"));
    }
    CliError::setup_step_failed("environment", run.last_error()).with_detail(run.lines.join("\n"))
}

/// Package, expected hash, and computed hash from uv's "Hash mismatch for `x`" report.
fn parse_uv_mismatch(lines: &[String]) -> (String, String, String) {
    let unknown = || "unknown".to_owned();
    let mut artifact = unknown();
    let (mut expected, mut computed) = (unknown(), unknown());
    for (index, line) in lines.iter().enumerate() {
        if let Some(rest) = line.split_once("Hash mismatch for `").map(|(_, rest)| rest) {
            artifact = rest.split('`').next().map_or_else(unknown, str::to_owned);
        }
        let next = || {
            lines
                .iter()
                .skip(index + 1)
                .map(|l| l.trim())
                .find(|l| !l.is_empty())
                .map_or_else(unknown, str::to_owned)
        };
        match line.trim() {
            "Expected:" => expected = next(),
            "Computed:" => computed = next(),
            _ => {}
        }
    }
    (artifact, expected, computed)
}

// -- step 4: models ---------------------------------------------------------------------------

fn step_models(
    ctx: &Ctx,
    run_tmp: &Path,
    log: &Arc<LogSink>,
    pins: &PinSet,
) -> Result<String, CliError> {
    let data = &ctx.data;
    struct Todo<'a> {
        repo: &'a str,
        revision: &'a str,
        dir: &'a str,
        path: &'a str,
        size: u64,
        sha256: &'a str,
    }
    let mut todo = Vec::new();
    for repo in &pins.pins.huggingface {
        for file in &repo.files {
            check_interrupt()?;
            let final_path = DataDir::join_pin_path(&data.models().join(&repo.dir), &file.path);
            if file_matches(&final_path, file.size, &file.sha256) {
                continue;
            }
            todo.push(Todo {
                repo: &repo.repo,
                revision: &repo.revision,
                dir: &repo.dir,
                path: &file.path,
                size: file.size,
                sha256: &file.sha256,
            });
        }
    }
    let staging_root = data.staging().join("models");
    if todo.is_empty() {
        let _ = fs::remove_dir_all(&staging_root);
        return Ok("ok (no network used)".to_owned());
    }

    let mut session = start_worker(ctx, run_tmp, Mode::Download, log, "models")?;
    for item in &todo {
        check_interrupt()?;
        let local_dir = staging_root.join(item.dir);
        let expected = DataDir::join_pin_path(&local_dir, item.path);
        let request = json!({
            "repo": item.repo,
            "revision": item.revision,
            "file": item.path,
            "local_dir": local_dir.to_string_lossy(),
        });
        let reply = session
            .request("download", request, None)
            .map_err(|err| worker_failure("models", err))?;
        let returned = reply
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !same_path(Path::new(returned), &expected) {
            return Err(CliError::setup_step_failed(
                "models",
                format!("the worker returned a path outside the staging directory: {returned}"),
            ));
        }
        let (size, actual) = (
            fs::metadata(&expected).map(|m| m.len()).ok(),
            sha256_file(&expected),
        );
        let actual = match (size, actual) {
            (Some(size), Ok(hash)) if size == item.size && hash == item.sha256 => hash,
            (_, Ok(hash)) => {
                let _ = fs::remove_file(&expected);
                return Err(CliError::hash_mismatch(
                    &format!("{}/{}", item.repo, item.path),
                    item.sha256,
                    &hash,
                ));
            }
            (_, Err(err)) => {
                return Err(CliError::setup_step_failed(
                    "models",
                    format!(
                        "cannot read the downloaded file {}/{}: {err}",
                        item.repo, item.path
                    ),
                ));
            }
        };
        let final_path = DataDir::join_pin_path(&data.models().join(item.dir), item.path);
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| CliError::data_dir_unwritable(data.root(), err))?;
        }
        fs::rename(&expected, &final_path).map_err(|err| {
            CliError::setup_step_failed(
                "models",
                format!("cannot move {} into place: {err}", item.path),
            )
        })?;
        log.file_only(&format!(
            "setup: verified {}/{} sha256 {actual}",
            item.repo, item.path
        ));
    }
    session.shutdown();
    let _ = fs::remove_dir_all(&staging_root);
    Ok("ok (downloaded)".to_owned())
}

fn file_matches(path: &Path, size: u64, sha256: &str) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() == size)
        && sha256_file(path).is_ok_and(|hash| hash == sha256)
}

/// Compare paths after making them absolute and folding ASCII case.
fn same_path(a: &Path, b: &Path) -> bool {
    let fold = |p: &Path| {
        std::path::absolute(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .to_ascii_lowercase()
    };
    fold(a) == fold(b)
}

// -- step 5: voices ---------------------------------------------------------------------------

fn step_voices(
    ctx: &Ctx,
    run_tmp: &Path,
    log: &Arc<LogSink>,
    pins: &PinSet,
) -> Result<String, CliError> {
    let data = &ctx.data;
    let recorded = read_record(data) == Record::Recorded(pins.id.clone());
    let todo: Vec<&str> = CLIPS
        .into_iter()
        .filter(|name| !(recorded && data.clip(name).is_file()))
        .collect();
    if todo.is_empty() {
        return Ok("ok (all present)".to_owned());
    }

    let mut renderer = ClipRenderer {
        session: start_worker(ctx, run_tmp, Mode::Offline, log, "voices")?,
        pins,
        kokoro_dir: data.models().join(KOKORO_DIR),
        turbo_dir: data.models().join(TURBO_DIR),
        builtin_ready: false,
    };
    let text = pins.pins.voices.reference_text.as_str();
    let doubled = format!("{text} {text}");
    let min_samples = usize::try_from(pins.pins.voices.min_samples).unwrap_or(usize::MAX);

    for name in &todo {
        check_interrupt()?;
        // A clip shorter than the minimum is rendered once more with the reference text twice.
        let mut samples = renderer.render(name, text)?;
        if samples.len() < min_samples {
            samples = renderer.render(name, &doubled)?;
        }
        if samples.len() < min_samples {
            return Err(CliError::setup_step_failed(
                "voices",
                format!(
                    "the {name} clip has {} samples after a second render; it needs at least {min_samples}",
                    samples.len()
                ),
            ));
        }
        let staged = data.staging().join("clips").join(format!("{name}.wav"));
        publish_clip(data, name, &staged, &samples).map_err(|err| {
            CliError::setup_step_failed("voices", format!("cannot write the {name} clip: {err}"))
        })?;
    }
    renderer.session.shutdown();
    let _ = fs::remove_dir_all(data.staging().join("clips"));
    Ok(format!("ok (generated {})", todo.join(", ")))
}

/// Write the clip to staging, then move it into `voices\` (RULE-002).
fn publish_clip(data: &DataDir, name: &str, staged: &Path, samples: &[i16]) -> std::io::Result<()> {
    if let Some(parent) = staged.parent() {
        fs::create_dir_all(parent)?;
    }
    write_wav_pcm16(staged, SAMPLE_RATE, samples)?;
    fs::create_dir_all(data.voices())?;
    fs::rename(staged, data.clip(name))
}

/// One worker session that renders the reference clips.
struct ClipRenderer<'a> {
    session: WorkerSession,
    pins: &'a PinSet,
    kokoro_dir: PathBuf,
    turbo_dir: PathBuf,
    builtin_ready: bool,
}

impl ClipRenderer<'_> {
    /// Render `text` with the voice of clip `name` and return 16-bit samples.
    fn render(&mut self, name: &str, text: &str) -> Result<Vec<i16>, CliError> {
        let voices = &self.pins.pins.voices;
        let reply = match name {
            "female" | "male" => {
                let voice = if name == "female" {
                    &voices.female
                } else {
                    &voices.male
                };
                let request = json!({
                    "model_dir": self.kokoro_dir.to_string_lossy(),
                    "voice": voice.voice,
                    "text": text,
                    // The CPU render does not depend on free VRAM, which other programs may hold.
                    "device": "cpu",
                });
                self.session.request("kokoro_clip", request, None)
            }
            _ => {
                self.prepare_builtin()?;
                let request = json!({
                    "chunk": 1,
                    "total": 1,
                    "voice_id": "builtin",
                    "text": text,
                    "seed": voices.builtin.seed,
                });
                self.session.request("synth", request, None)
            }
        }
        .map_err(|err| worker_failure("voices", err))?;
        clip_samples(&reply)
    }

    /// Load Turbo and select its bundled voice, once per session. When the GPU has too little
    /// free memory the one-time render moves to the CPU, which gives an equally valid clip.
    fn prepare_builtin(&mut self) -> Result<(), CliError> {
        if self.builtin_ready {
            return Ok(());
        }
        let load = |device: &str| json!({ "model_dir": self.turbo_dir.to_string_lossy(), "device": device });
        match self.session.request("load", load("auto"), None) {
            Ok(_) => {}
            Err(WorkerError::Op { code, .. }) if code == "out_of_memory" => {
                say(format_args!(
                    "setup: the GPU has too little free memory for the model; rendering the builtin clip on the CPU"
                ));
                self.session
                    .request("load", load("cpu"), None)
                    .map_err(|err| worker_failure("voices", err))?;
            }
            Err(err) => return Err(worker_failure("voices", err)),
        }
        let voice = json!({ "voice_id": "builtin", "clip": Value::Null });
        self.session
            .request("voice", voice, None)
            .map_err(|err| worker_failure("voices", err))?;
        self.builtin_ready = true;
        Ok(())
    }
}

/// 16-bit samples from a `kokoro_clip` or `synth` reply.
fn clip_samples(reply: &Value) -> Result<Vec<i16>, CliError> {
    let bad = |detail: &str| {
        CliError::setup_step_failed(
            "voices",
            format!("the worker sent an invalid audio reply: {detail}"),
        )
    };
    if reply.get("sample_rate").and_then(Value::as_u64) != Some(u64::from(SAMPLE_RATE)) {
        return Err(bad("the sample rate is not 24000"));
    }
    let count = reply
        .get("samples")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("no sample count"))?;
    let hex = reply
        .get("pcm_f32le_hex")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("no audio data"))?;
    let floats = decode_f32_hex(hex, count).map_err(|err| bad(&err))?;
    Ok(floats.into_iter().map(f32_to_pcm16).collect())
}

// -- step 6: record ---------------------------------------------------------------------------

fn step_record(data: &DataDir, pins: &PinSet) -> Result<(), CliError> {
    let mut clips = serde_json::Map::new();
    for name in CLIPS {
        let path = data.clip(name);
        let sha = sha256_file(&path).map_err(|err| {
            CliError::setup_step_failed("record", format!("cannot hash the {name} clip: {err}"))
        })?;
        let info = inspect_clip(&path).map_err(|err| {
            CliError::setup_step_failed("record", format!("cannot read the {name} clip: {err}"))
        })?;
        clips.insert(
            name.to_owned(),
            json!({ "sha256": sha, "samples": info.frames }),
        );
    }
    let record = json!({
        "format": 1,
        "pin_set_id": pins.id,
        "narrate_version": env!("CARGO_PKG_VERSION"),
        "completed_unix": unix_seconds(),
        "clips": clips,
    });
    let text = serde_json::to_string(&record).map_err(|err| CliError::internal(err.to_string()))?;
    write_atomic(&data.setup_json(), text.as_bytes()).map_err(|err| {
        CliError::setup_step_failed("record", format!("cannot write setup.json: {err}"))
    })
}

// -- shared worker plumbing -------------------------------------------------------------------

fn start_worker(
    ctx: &Ctx,
    run_tmp: &Path,
    mode: Mode,
    log: &Arc<LogSink>,
    step: &str,
) -> Result<WorkerSession, CliError> {
    WorkerSession::start(ctx, run_tmp, mode, StderrTarget::Shared(Arc::clone(log)))
        .map_err(|err| worker_failure(step, err))
}

/// Every worker error during setup is `setup-step-failed` at the step, except an interrupt.
fn worker_failure(step: &str, err: WorkerError) -> CliError {
    match err {
        WorkerError::Interrupted => CliError::setup_interrupted(),
        WorkerError::Op {
            code,
            message,
            traceback,
        } => {
            let mut error = CliError::setup_step_failed(step, format!("{message} ({code})"));
            error.details.extend(traceback_details(&traceback));
            error
        }
        WorkerError::Exited { code } => {
            let code = code.map_or_else(|| "unknown".to_owned(), |c| c.to_string());
            CliError::setup_step_failed(
                step,
                format!("the worker exited unexpectedly (exit code {code})"),
            )
        }
        WorkerError::Protocol(detail) => CliError::setup_step_failed(
            step,
            format!("the worker sent an invalid message: {detail}"),
        ),
        WorkerError::Start(detail) => {
            CliError::setup_step_failed(step, format!("could not start the worker: {detail}"))
        }
        WorkerError::Timeout => {
            CliError::setup_step_failed(step, "the worker did not answer in time")
        }
    }
}
