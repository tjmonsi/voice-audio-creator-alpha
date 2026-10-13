//! `narrate <file>`: script to WAV (spec-narrate-generation.md).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use narrate_core::{
    Chunk, ClipError, ClipInfo, MAX_CHUNK_CHARS, SAMPLE_RATE, ScriptError, ScriptKind,
    WAV_SIZE_LIMIT, chunk_script, f32_to_pcm16, inspect_clip, is_too_short, load_script,
    needs_trim, unknown_tags, wav_file_size, write_clip_prefix, write_wav_pcm16,
};
use serde_json::{Value, json};

use crate::cli::{Device, VoiceChoice};
use crate::codec::decode_f32_hex;
use crate::ctx::Ctx;
use crate::error::CliError;
use crate::fsutil::{RunTemp, clean_old_worker_logs, clean_stale_staging_clips, clean_stale_tmp};
use crate::output::{result_line, say};
use crate::pins::PinSet;
use crate::ready::Readiness;
use crate::worker::{Mode, StderrTarget, WorkerError, WorkerSession, traceback_details};

/// Samples at or above this count mean the chunk reached the model's 40 s cap (RULE-005).
const MODEL_LIMIT_SAMPLES: u64 = 948_000;

/// Arguments of a generation run, as parsed from the command line.
pub struct Options {
    pub file: PathBuf,
    pub output: Option<PathBuf>,
    pub voice: VoiceChoice,
    pub device: Device,
}

/// Everything the preconditions establish before a worker starts.
struct Plan {
    input: PathBuf,
    output_typed: PathBuf,
    output: PathBuf,
    clip: Option<(PathBuf, ClipInfo)>,
}

/// Removes a trimmed clip when the run ends, whatever the outcome.
struct TrimmedClip(Option<PathBuf>);

impl Drop for TrimmedClip {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

pub fn run(ctx: &Ctx, options: &Options) -> Result<(), CliError> {
    let plan = check_paths(options)?;

    // Precondition 8: readiness, before the script is read and before any worker starts.
    let pins = PinSet::load(&ctx.hooks)?;
    Readiness::check(&ctx.data, &pins).require_ready(&ctx.data)?;
    clean_stale_tmp(&ctx.data);
    clean_stale_staging_clips(&ctx.data);
    clean_old_worker_logs(&ctx.data);
    ctx.verbose_line(format_args!(
        "verbose: data directory {}",
        ctx.data.root().display()
    ));

    // Precondition 9 and chunking.
    let script = load_script(&options.file).map_err(|err| script_error(&options.file, err))?;
    let chunks = chunk_script(&script, MAX_CHUNK_CHARS);
    ctx.verbose_line(format_args!("verbose: chunks {}", chunks.len()));

    // Precondition 10: the output directory accepts a new file.
    let probe = temp_output_path(&plan.output);
    fs::File::create(&probe)
        .and_then(|_| fs::remove_file(&probe))
        .map_err(|err| CliError::output_not_writable(&plan.output_typed, err))?;

    let run_tmp = RunTemp::create(&ctx.data).map_err(|err| {
        CliError::internal(format!("cannot create the run temporary directory: {err}"))
    })?;
    let (voice_id, voice_clip, _trimmed) = announce_voice(ctx, options, &plan)?;
    warn_unknown_tags(&chunks);

    let mut session = WorkerSession::start(
        ctx,
        run_tmp.path(),
        Mode::Offline,
        StderrTarget::PerWorkerLog,
    )
    .map_err(|err| map_worker_error(err, &Phase::Hello, None))?;
    let log_path = session.log_path().map(Path::to_path_buf);
    let outcome = synthesize(
        ctx,
        options,
        &mut session,
        &chunks,
        &voice_id,
        voice_clip.as_deref(),
        log_path.as_deref(),
    );
    let audio = match outcome {
        Ok(audio) => audio,
        Err(err) => {
            session.abort(Duration::from_secs(3));
            return Err(err);
        }
    };
    session.shutdown();
    write_output(&plan, &audio)?;
    if let Some(log) = &log_path {
        let _ = fs::remove_file(log);
    }
    Ok(())
}

// -- preconditions 1 to 7 ---------------------------------------------------------------------

fn check_paths(options: &Options) -> Result<Plan, CliError> {
    let typed = &options.file;
    ScriptKind::from_path(typed).map_err(|_| CliError::input_extension(typed))?;
    let metadata = fs::metadata(typed).map_err(|err| match err.kind() {
        io::ErrorKind::NotFound => CliError::input_not_found(typed),
        _ => CliError::input_unreadable(typed, err),
    })?;
    if !metadata.is_file() {
        return Err(CliError::input_not_a_file(typed));
    }
    let input = std::path::absolute(typed)?;
    let output_typed = options
        .output
        .clone()
        .unwrap_or_else(|| typed.with_extension("wav"));
    let output = std::path::absolute(&output_typed)?;

    let typed_parent = output_typed
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = output
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    if !parent.is_dir() {
        return Err(CliError::output_dir_missing(typed_parent));
    }
    if output.is_dir() {
        return Err(CliError::output_is_directory(&output_typed));
    }
    if let Ok(existing) = fs::metadata(&output) {
        let same = fs::canonicalize(&output)
            .ok()
            .zip(fs::canonicalize(typed).ok())
            .is_some_and(|(a, b)| a == b);
        if same {
            return Err(CliError::output_is_input(&output_typed));
        }
        if existing.permissions().readonly() {
            return Err(CliError::output_not_writable(
                &output_typed,
                "the file is read-only",
            ));
        }
    }
    let clip = match &options.voice {
        VoiceChoice::File(path) => Some(check_clip(path)?),
        _ => None,
    };
    Ok(Plan {
        input,
        output_typed,
        output,
        clip,
    })
}

fn check_clip(typed: &Path) -> Result<(PathBuf, ClipInfo), CliError> {
    let info = inspect_clip(typed).map_err(|err| match err {
        ClipError::NotFound { .. } => CliError::voice_file_not_found(typed),
        ClipError::Unreadable { detail, .. } => CliError::voice_file_unreadable(typed, &detail),
        ClipError::Unsupported { detail, .. } => CliError::voice_file_unsupported(typed, &detail),
    })?;
    if is_too_short(&info) {
        return Err(CliError::voice_file_too_short(typed, info.duration_secs()));
    }
    Ok((std::path::absolute(typed)?, info))
}

fn script_error(typed: &Path, err: ScriptError) -> CliError {
    match err {
        ScriptError::UnsupportedExtension { .. } => CliError::input_extension(typed),
        ScriptError::NotFound { .. } => CliError::input_not_found(typed),
        ScriptError::NotAFile { .. } => CliError::input_not_a_file(typed),
        ScriptError::Io { source, .. } => CliError::input_unreadable(typed, source),
        ScriptError::InvalidUtf8 { offset, .. } => CliError::input_encoding(typed, offset),
        ScriptError::Empty { .. } => CliError::script_empty(typed),
    }
}

// -- announcements ----------------------------------------------------------------------------

/// Print the `voice:` and `notice:` lines; returns the voice id, the clip for the worker, and the
/// guard of a trimmed clip.
fn announce_voice(
    ctx: &Ctx,
    options: &Options,
    plan: &Plan,
) -> Result<(String, Option<PathBuf>, TrimmedClip), CliError> {
    let data = &ctx.data;
    match &options.voice {
        VoiceChoice::Female | VoiceChoice::Male => {
            let (id, clip) = if matches!(options.voice, VoiceChoice::Female) {
                ("female", data.clip("female"))
            } else {
                ("male", data.clip("male"))
            };
            say(format_args!("voice: {id} ({})", clip.display()));
            Ok((id.to_owned(), Some(clip), TrimmedClip(None)))
        }
        VoiceChoice::Builtin => {
            say(format_args!("voice: builtin"));
            Ok(("builtin".to_owned(), None, TrimmedClip(None)))
        }
        VoiceChoice::File(_) => {
            let (absolute, info) = plan
                .clip
                .as_ref()
                .ok_or_else(|| CliError::internal("the custom clip was not inspected"))?;
            say(format_args!("voice: file ({})", absolute.display()));
            if !needs_trim(info) {
                return Ok(("file".to_owned(), Some(absolute.clone()), TrimmedClip(None)));
            }
            say(format_args!(
                "notice: {} is {:.1} s long; only the first 10 s are used",
                absolute.display(),
                info.duration_secs()
            ));
            let staged = data
                .staging()
                .join(format!("voice-{}.wav", std::process::id()));
            fs::create_dir_all(data.staging()).map_err(|err| {
                CliError::internal(format!("cannot create the staging directory: {err}"))
            })?;
            let guard = TrimmedClip(Some(staged.clone()));
            write_clip_prefix(absolute, &staged, 10 * u64::from(info.sample_rate))
                .map_err(|err| CliError::internal(format!("cannot trim the voice clip: {err}")))?;
            Ok(("file".to_owned(), Some(staged), guard))
        }
    }
}

fn warn_unknown_tags(chunks: &[Chunk]) {
    let mut seen: Vec<String> = Vec::new();
    for chunk in chunks {
        for tag in unknown_tags(&chunk.text) {
            if !seen.contains(&tag) {
                say(format_args!(
                    "warning: unknown tag {tag} will be read as text"
                ));
                seen.push(tag);
            }
        }
    }
}

// -- the worker session -----------------------------------------------------------------------

/// What the run was doing when a worker error arrived.
enum Phase {
    Hello,
    Load,
    Voice(String),
    Synth { chunk: usize, total: usize },
}

struct Audio {
    samples: Vec<i16>,
    synth_seconds: f64,
}

fn synthesize(
    ctx: &Ctx,
    options: &Options,
    session: &mut WorkerSession,
    chunks: &[Chunk],
    voice_id: &str,
    clip: Option<&Path>,
    log: Option<&Path>,
) -> Result<Audio, CliError> {
    let map = |phase: &Phase, err: WorkerError| map_worker_error(err, phase, log);

    let device = match options.device {
        Device::Auto => "auto",
        Device::Cuda => "cuda",
        Device::Cpu => "cpu",
    };
    let model_dir = ctx.data.models().join("chatterbox-turbo");
    let loaded = session
        .request(
            "load",
            json!({ "model_dir": model_dir.to_string_lossy(), "device": device }),
            None,
        )
        .map_err(|err| map(&Phase::Load, err))?;
    ctx.verbose_line(format_args!(
        "verbose: model load {:.1} s",
        loaded
            .get("load_seconds")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    ));
    if loaded
        .get("fallback")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        say(format_args!(
            "warning: CUDA is not available; falling back to CPU (generation will be slower)"
        ));
    }
    match loaded.get("device").and_then(Value::as_str) {
        Some("cuda") => {
            let name = loaded
                .get("device_name")
                .and_then(Value::as_str)
                .unwrap_or("unknown device");
            say(format_args!("device: cuda ({name})"));
        }
        Some("cpu") => say(format_args!("device: cpu")),
        other => {
            return Err(CliError::worker_protocol(format!(
                "load answered with device {other:?}"
            )));
        }
    }

    let clip_value = clip.map_or(Value::Null, |p| {
        Value::String(p.to_string_lossy().into_owned())
    });
    session
        .request(
            "voice",
            json!({ "voice_id": voice_id, "clip": clip_value }),
            None,
        )
        .map_err(|err| map(&Phase::Voice(voice_id.to_owned()), err))?;

    let limit = ctx.hooks.wav_limit.unwrap_or(WAV_SIZE_LIMIT);
    let total = chunks.len();
    let mut audio = Audio {
        samples: Vec::new(),
        synth_seconds: 0.0,
    };
    for chunk in chunks {
        let phase = Phase::Synth {
            chunk: chunk.number,
            total,
        };
        ctx.verbose_line(format_args!(
            "verbose: chunk {}/{total} text: {}",
            chunk.number, chunk.text
        ));
        let request = json!({
            "chunk": chunk.number,
            "total": total,
            "voice_id": voice_id,
            "text": chunk.text,
            "seed": chunk.number,
        });
        let reply = session
            .request("synth", request, None)
            .map_err(|err| map(&phase, err))?;
        let added = append_chunk(&mut audio, &reply, chunk.number)?;
        ctx.verbose_line(format_args!(
            "verbose: chunk {}/{total} samples={added}",
            chunk.number
        ));
        if wav_file_size(audio.samples.len() as u64) > limit {
            return Err(CliError::wav_too_large(limit));
        }
        if u64::from(added) >= MODEL_LIMIT_SAMPLES {
            say(format_args!(
                "warning: chunk {}/{total} reached the 40 s model limit; its speech may be cut off",
                chunk.number
            ));
        }
        let silence = chunk.pause_after.samples();
        audio
            .samples
            .resize(audio.samples.len() + silence as usize, 0);
        ctx.verbose_line(format_args!(
            "verbose: silence after chunk {} samples={silence}",
            chunk.number
        ));
        if wav_file_size(audio.samples.len() as u64) > limit {
            return Err(CliError::wav_too_large(limit));
        }
        say(format_args!("chunk {}/{total} done", chunk.number));
    }
    Ok(audio)
}

/// Validate a `synth` reply and append its samples; returns how many were added.
fn append_chunk(audio: &mut Audio, reply: &Value, chunk: usize) -> Result<u32, CliError> {
    let protocol = |detail: String| CliError::worker_protocol(detail);
    if reply.get("chunk").and_then(Value::as_u64) != Some(chunk as u64) {
        return Err(protocol(format!("synth reply is not for chunk {chunk}")));
    }
    if reply.get("sample_rate").and_then(Value::as_u64) != Some(u64::from(SAMPLE_RATE)) {
        return Err(protocol("the sample rate is not 24000".to_owned()));
    }
    let count = reply
        .get("samples")
        .and_then(Value::as_u64)
        .filter(|&n| n >= 1)
        .ok_or_else(|| protocol("synth reply has no samples".to_owned()))?;
    let hex = reply
        .get("pcm_f32le_hex")
        .and_then(Value::as_str)
        .ok_or_else(|| protocol("synth reply has no audio data".to_owned()))?;
    let floats = decode_f32_hex(hex, count).map_err(protocol)?;
    audio.samples.extend(floats.into_iter().map(f32_to_pcm16));
    audio.synth_seconds += reply
        .get("synth_seconds")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .max(0.0);
    u32::try_from(count).map_err(|_| CliError::worker_protocol("synth reply is too long"))
}

fn map_worker_error(err: WorkerError, phase: &Phase, log: Option<&Path>) -> CliError {
    let log_path = log.map_or_else(|| PathBuf::from("the worker log"), Path::to_path_buf);
    let while_text = match phase {
        Phase::Hello => "while hello".to_owned(),
        Phase::Load => "while load".to_owned(),
        Phase::Voice(_) => "while voice".to_owned(),
        Phase::Synth { chunk, total } => format!("during chunk {chunk} of {total}"),
    };
    match err {
        WorkerError::Interrupted => CliError::interrupted(),
        WorkerError::Exited { code } => CliError::worker_exited(code, &while_text, &log_path),
        WorkerError::Protocol(detail) => CliError::worker_protocol(detail),
        WorkerError::Start(detail) => CliError::worker_start(detail),
        WorkerError::Timeout => CliError::worker_protocol("no reply within the time limit"),
        WorkerError::Op {
            code,
            message,
            traceback,
        } => {
            let mut error = op_error(&code, &message, phase);
            error.details.extend(traceback_details(&traceback));
            error
        }
    }
}

/// Map a worker error code to the catalog (spec-narrate-worker.md, Error handling).
fn op_error(code: &str, message: &str, phase: &Phase) -> CliError {
    match (code, phase) {
        ("cuda_unavailable", _) => CliError::cuda_unavailable(),
        ("watermarker_unavailable", _) => CliError::watermark_unavailable(),
        ("model_load_failed", _) => CliError::model_load(message),
        ("out_of_memory", Phase::Synth { chunk, total }) => {
            CliError::gpu_out_of_memory(*chunk, *total)
        }
        ("out_of_memory", _) => CliError::gpu_out_of_memory_loading(),
        ("builtin_unavailable" | "voice_failed", Phase::Voice(voice)) => {
            CliError::voice_prepare(voice, message)
        }
        ("synth_failed" | "test_failure", Phase::Synth { chunk, total }) => {
            CliError::chunk_failed(*chunk, *total, message)
        }
        (_, Phase::Synth { chunk, total }) if code != "bad_request" => {
            CliError::chunk_failed(*chunk, *total, message)
        }
        (_, Phase::Voice(voice)) if code != "bad_request" => {
            CliError::voice_prepare(voice, message)
        }
        (_, Phase::Load) if code != "bad_request" => CliError::model_load(message),
        _ => CliError::worker_protocol(format!("the worker answered {code}: {message}")),
    }
}

// -- output -----------------------------------------------------------------------------------

fn temp_output_path(output: &Path) -> PathBuf {
    let mut name = output
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(format!(".narrate-{}.tmp", std::process::id()));
    output.with_file_name(name)
}

/// Write the temporary WAV, rename it over the output, and print `done:` and the path.
fn write_output(plan: &Plan, audio: &Audio) -> Result<(), CliError> {
    let temp = temp_output_path(&plan.output);
    let result = write_wav_pcm16(&temp, SAMPLE_RATE, &audio.samples)
        .and_then(|()| fs::rename(&temp, &plan.output));
    if let Err(err) = result {
        let _ = fs::remove_file(&temp);
        return Err(if is_permission_problem(&err) {
            CliError::output_not_writable(&plan.output_typed, err)
        } else {
            CliError::output_write(&plan.output_typed, err)
        });
    }
    let seconds = audio.samples.len() as f64 / f64::from(SAMPLE_RATE);
    let rtf = if seconds > 0.0 {
        audio.synth_seconds / seconds
    } else {
        0.0
    };
    say(format_args!(
        "done: {} ({seconds:.1} s audio, {} samples, synthesis {:.1} s, rtf {rtf:.2})",
        plan.output.display(),
        audio.samples.len(),
        audio.synth_seconds
    ));
    result_line(format_args!("{}", plan.output.display()));
    let _ = &plan.input;
    Ok(())
}

/// Access denied and sharing violation both mean the user can fix the destination.
fn is_permission_problem(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::PermissionDenied || matches!(err.raw_os_error(), Some(5 | 32 | 33))
}
