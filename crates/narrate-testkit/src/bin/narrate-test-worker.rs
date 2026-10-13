//! Fake worker for the narrate tests: the JSON-lines protocol of spec-narrate-worker.md
//! (Protocol schemas and Requirement 9), driven by `FAKE_WORKER_*` variables. The knob list is
//! documented at the top of `crates/narrate-testkit/tests/smoke.rs`, which also tests it.
//!
//! One JSON request per stdin line, exactly one JSON reply per request on stdout, in order.
//! A banner `narrate-test-worker: started pid <pid>` goes to stderr at start. The worker exits
//! 0 on end-of-file of stdin (also while a `synth` delay is running) and right after replying to
//! `shutdown`.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Map, Value, json};

const SAMPLE_RATE: u64 = 24_000;
const KNOWN_OPS: [&str; 8] =
    ["hello", "probe", "load", "voice", "synth", "kokoro_clip", "download", "shutdown"];

/// Set by the reader thread when stdin reaches end-of-file.
static STDIN_CLOSED: AtomicBool = AtomicBool::new(false);

enum Msg {
    Line(String),
    Eof,
}

#[derive(Clone, Copy, PartialEq)]
enum DeviceMode {
    Cuda,
    CpuFallback,
    Cpu,
    CudaUnavailable,
}

struct Config {
    log: Option<String>,
    env_dump: Option<String>,
    device: DeviceMode,
    device_name: String,
    probe_cuda: bool,
    samples_per_char: u64,
    synth_samples: Option<u64>,
    clip_samples: u64,
    download_dir: Option<PathBuf>,
    download_override: Option<String>,
    synth_delay_ms: u64,
    protocol: u64,
    traceback: Option<String>,
    fail_op: Option<(String, String)>,
    wrong_id_op: Option<String>,
    garbage_op: Option<String>,
    exit_op: Option<String>,
    fail_chunk: Option<u64>,
    crash_chunk: Option<u64>,
    watermark_fail: bool,
}

/// A variable's value, or `None` when it is unset or empty.
fn var(name: &str) -> Option<String> {
    std::env::var_os(name)
        .map(|v| v.to_string_lossy().into_owned())
        .filter(|v| !v.is_empty())
}

fn var_u64(name: &str) -> Option<u64> {
    var(name).and_then(|v| v.trim().parse().ok())
}

impl Config {
    fn from_env() -> Config {
        let device = match var("FAKE_WORKER_DEVICE").as_deref() {
            None | Some("cuda") => DeviceMode::Cuda,
            Some("cpu-fallback") => DeviceMode::CpuFallback,
            Some("cpu") => DeviceMode::Cpu,
            Some("cuda-unavailable") => DeviceMode::CudaUnavailable,
            Some(other) => {
                note(&format!("unknown FAKE_WORKER_DEVICE {other:?}, using cuda"));
                DeviceMode::Cuda
            }
        };
        Config {
            log: var("FAKE_WORKER_LOG"),
            env_dump: var("FAKE_WORKER_ENV_DUMP"),
            device,
            device_name: var("FAKE_WORKER_DEVICE_NAME").unwrap_or_else(|| "NVIDIA GeForce RTX 3050".to_string()),
            probe_cuda: var("FAKE_WORKER_PROBE_CUDA").as_deref() != Some("0"),
            samples_per_char: var_u64("FAKE_WORKER_SAMPLES_PER_CHAR").unwrap_or(100),
            synth_samples: var_u64("FAKE_WORKER_SYNTH_SAMPLES"),
            clip_samples: var_u64("FAKE_WORKER_CLIP_SAMPLES").unwrap_or(240_000),
            download_dir: var("FAKE_WORKER_DOWNLOAD_DIR").map(PathBuf::from),
            download_override: var("FAKE_WORKER_DOWNLOAD_PATH_OVERRIDE"),
            synth_delay_ms: var_u64("FAKE_WORKER_SYNTH_DELAY_MS").unwrap_or(0),
            protocol: var_u64("FAKE_WORKER_PROTOCOL").unwrap_or(1),
            traceback: var("FAKE_WORKER_TRACEBACK"),
            fail_op: var("FAKE_WORKER_FAIL_OP")
                .and_then(|v| v.split_once(':').map(|(op, code)| (op.to_string(), code.to_string()))),
            wrong_id_op: var("FAKE_WORKER_WRONG_ID_OP"),
            garbage_op: var("FAKE_WORKER_GARBAGE_OP"),
            exit_op: var("FAKE_WORKER_EXIT_OP"),
            fail_chunk: var_u64("NARRATE_TEST_FAIL_CHUNK"),
            crash_chunk: var_u64("NARRATE_TEST_CRASH_CHUNK"),
            watermark_fail: var("NARRATE_TEST_WATERMARK_FAIL").as_deref() == Some("1"),
        }
    }
}

/// A diagnostic line on stderr (never on stdout, which carries the protocol).
fn note(text: &str) {
    let _ = writeln!(std::io::stderr(), "narrate-test-worker: {text}");
}

struct Failure {
    code: String,
    message: String,
}

fn failure(code: &str, message: impl Into<String>) -> Failure {
    Failure { code: code.to_string(), message: message.into() }
}

type Reply = Result<Map<String, Value>, Failure>;

#[derive(Default)]
struct State {
    loaded: bool,
    voices: HashSet<String>,
}

// ---------------------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------------------

/// Writes one line to stdout in a single write; a broken pipe ends the worker.
fn send_line(text: &str) {
    let mut buf = Vec::with_capacity(text.len() + 1);
    buf.extend_from_slice(text.as_bytes());
    buf.push(b'\n');
    let mut out = std::io::stdout().lock();
    if out.write_all(&buf).and_then(|_| out.flush()).is_err() {
        exit(0);
    }
}

fn append_log(cfg: &Config, entry: &Value) {
    let Some(path) = &cfg.log else { return };
    let mut text = entry.to_string();
    text.push('\n');
    let result = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(text.as_bytes()));
    if let Err(e) = result {
        note(&format!("cannot write the log {path}: {e}"));
    }
}

#[derive(Serialize)]
struct EnvDump {
    pid: u32,
    cwd: String,
    temp_exists: bool,
    env: BTreeMap<String, String>,
}

fn write_env_dump(cfg: &Config, pid: u32) {
    let Some(template) = &cfg.env_dump else { return };
    let path = template.replace("{pid}", &pid.to_string());
    let dump = EnvDump {
        pid,
        cwd: std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default(),
        temp_exists: std::env::var_os("TEMP").is_some_and(|t| Path::new(&t).is_dir()),
        env: std::env::vars_os()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
            .collect(),
    };
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = Path::new(&path).parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string(&dump).map_err(std::io::Error::other)?;
        fs::write(&path, text)
    })();
    if let Err(e) = result {
        note(&format!("cannot write the environment dump {path}: {e}"));
    }
}

// ---------------------------------------------------------------------------------------
// Audio helpers
// ---------------------------------------------------------------------------------------

const HEX: &[u8; 16] = b"0123456789abcdef";
const PERIOD: usize = 120;

/// `pcm_f32le_hex` of `samples` samples of a quiet sine tone (every value finite, |x| <= 0.25).
fn pcm_hex(samples: u64) -> String {
    let mut period = Vec::with_capacity(PERIOD * 8);
    for i in 0..PERIOD {
        let value = 0.25_f32 * (2.0 * std::f32::consts::PI * i as f32 / PERIOD as f32).sin();
        for byte in value.to_le_bytes() {
            period.push(HEX[(byte >> 4) as usize]);
            period.push(HEX[(byte & 0x0f) as usize]);
        }
    }
    let total = samples as usize;
    let mut out = Vec::with_capacity(total * 8);
    for _ in 0..total / PERIOD {
        out.extend_from_slice(&period);
    }
    out.extend_from_slice(&period[..(total % PERIOD) * 8]);
    String::from_utf8(out).unwrap_or_default()
}

/// `(exists, frames, sample_rate)` read from the RIFF headers of a WAV file. When the data chunk
/// declares more bytes than the file holds, the bytes actually present are counted.
fn clip_facts(path: &str) -> (bool, Option<u64>, Option<u32>) {
    let p = Path::new(path);
    if !p.is_file() {
        return (false, None, None);
    }
    let Ok(bytes) = fs::read(p) else { return (true, None, None) };
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return (true, None, None);
    }
    let mut at = 12usize;
    let mut format: Option<(u16, u32, u16)> = None;
    let mut data_len: Option<u64> = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        let body = at + 8;
        if id == b"fmt " && body + 16 <= bytes.len() {
            let channels = u16::from_le_bytes([bytes[body + 2], bytes[body + 3]]);
            let rate = u32::from_le_bytes([bytes[body + 4], bytes[body + 5], bytes[body + 6], bytes[body + 7]]);
            let bits = u16::from_le_bytes([bytes[body + 14], bytes[body + 15]]);
            format = Some((channels, rate, bits));
        } else if id == b"data" {
            data_len = Some(size.min(bytes.len() - body) as u64);
        }
        if format.is_some() && data_len.is_some() {
            break;
        }
        at = body.saturating_add(size).saturating_add(size & 1);
    }
    match (format, data_len) {
        (Some((channels, rate, bits)), Some(len)) => {
            let frame_bytes = u64::from(channels) * u64::from(bits).div_ceil(8);
            let frames = len.checked_div(frame_bytes);
            (true, frames, Some(rate))
        }
        (Some((_, rate, _)), None) => (true, None, Some(rate)),
        _ => (true, None, None),
    }
}

// ---------------------------------------------------------------------------------------
// Request handling
// ---------------------------------------------------------------------------------------

fn str_field<'a>(req: &'a Map<String, Value>, name: &str) -> Result<&'a str, Failure> {
    req.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| failure("bad_request", format!("field {name:?} must be a string")))
}

fn uint_field(req: &Map<String, Value>, name: &str) -> Result<u64, Failure> {
    req.get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| failure("bad_request", format!("field {name:?} must be a non-negative integer")))
}

fn one_of<'a>(req: &'a Map<String, Value>, name: &str, allowed: &[&str]) -> Result<&'a str, Failure> {
    let v = str_field(req, name)?;
    if allowed.contains(&v) {
        Ok(v)
    } else {
        Err(failure("bad_request", format!("field {name:?} must be one of {allowed:?}")))
    }
}

fn fields(pairs: Vec<(&str, Value)>) -> Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn op_hello(cfg: &Config) -> Reply {
    Ok(fields(vec![
        ("protocol", json!(cfg.protocol)),
        ("worker", json!("narrate-test-worker 0.1.0")),
        ("python", json!("3.11.14")),
    ]))
}

fn op_probe(cfg: &Config) -> Reply {
    Ok(fields(vec![
        ("cuda_available", json!(cfg.probe_cuda)),
        ("device_name", if cfg.probe_cuda { json!("Fake GPU") } else { Value::Null }),
        ("torch", json!("2.6.0+cu124")),
    ]))
}

fn op_load(cfg: &Config, state: &mut State, req: &Map<String, Value>) -> Reply {
    str_field(req, "model_dir")?;
    one_of(req, "device", &["auto", "cuda", "cpu"])?;
    if cfg.watermark_fail {
        return Err(failure("watermarker_unavailable", "the watermarker is unavailable (NARRATE_TEST_WATERMARK_FAIL=1)"));
    }
    let (device, name, fallback) = match cfg.device {
        DeviceMode::Cuda => ("cuda", json!(cfg.device_name), false),
        DeviceMode::CpuFallback => ("cpu", Value::Null, true),
        DeviceMode::Cpu => ("cpu", Value::Null, false),
        DeviceMode::CudaUnavailable => {
            return Err(failure("cuda_unavailable", "CUDA is not available (fake worker)"));
        }
    };
    state.loaded = true;
    Ok(fields(vec![
        ("device", json!(device)),
        ("device_name", name),
        ("fallback", json!(fallback)),
        ("load_seconds", json!(0.01)),
    ]))
}

fn op_voice(state: &mut State, req: &Map<String, Value>) -> Reply {
    let voice_id = str_field(req, "voice_id")?;
    if !state.loaded {
        return Err(failure("bad_request", "voice before load"));
    }
    match req.get("clip") {
        Some(Value::Null) if voice_id == "builtin" => {}
        Some(Value::String(_)) => {}
        _ => {
            return Err(failure("bad_request", "field \"clip\" must be a path (null only for builtin)"));
        }
    }
    state.voices.insert(voice_id.to_string());
    Ok(fields(vec![("voice_id", json!(voice_id))]))
}

fn op_synth(cfg: &Config, state: &State, req: &Map<String, Value>) -> Reply {
    let chunk = uint_field(req, "chunk")?;
    let total = uint_field(req, "total")?;
    let voice_id = str_field(req, "voice_id")?;
    let text = str_field(req, "text")?;
    if !req.get("seed").is_some_and(|s| s.is_i64() || s.is_u64()) {
        return Err(failure("bad_request", "field \"seed\" must be an integer"));
    }
    if chunk < 1 || total < chunk {
        return Err(failure("bad_request", "chunk must be >= 1 and total >= chunk"));
    }
    if !state.loaded || !state.voices.contains(voice_id) {
        return Err(failure("bad_request", format!("voice {voice_id:?} is not prepared")));
    }
    if cfg.fail_chunk == Some(chunk) {
        return Err(failure("test_failure", format!("failure injected for chunk {chunk} (NARRATE_TEST_FAIL_CHUNK)")));
    }
    let samples = cfg
        .synth_samples
        .unwrap_or_else(|| (text.chars().count() as u64).saturating_mul(cfg.samples_per_char).max(1));
    Ok(fields(vec![
        ("chunk", json!(chunk)),
        ("samples", json!(samples)),
        ("sample_rate", json!(SAMPLE_RATE)),
        ("synth_seconds", json!(0.5)),
        ("pcm_f32le_hex", json!(pcm_hex(samples))),
    ]))
}

fn op_kokoro_clip(cfg: &Config, req: &Map<String, Value>) -> Reply {
    str_field(req, "model_dir")?;
    one_of(req, "voice", &["af_heart", "am_michael"])?;
    str_field(req, "text")?;
    let requested = one_of(req, "device", &["auto", "cuda", "cpu"])?;
    let device = match (cfg.device, requested) {
        (DeviceMode::CudaUnavailable, "cuda") => {
            return Err(failure("cuda_unavailable", "CUDA is not available (fake worker)"));
        }
        (DeviceMode::Cuda, "auto" | "cuda") => "cuda",
        _ => "cpu",
    };
    Ok(fields(vec![
        ("samples", json!(cfg.clip_samples)),
        ("sample_rate", json!(SAMPLE_RATE)),
        ("device", json!(device)),
        ("pcm_f32le_hex", json!(pcm_hex(cfg.clip_samples))),
    ]))
}

fn op_download(cfg: &Config, req: &Map<String, Value>) -> Reply {
    let repo = str_field(req, "repo")?;
    str_field(req, "revision")?;
    let file = str_field(req, "file")?;
    let local_dir = str_field(req, "local_dir")?;
    if let Some(path) = &cfg.download_override {
        return Ok(fields(vec![("path", json!(path))]));
    }
    let fail = |why: String| failure("download_failed", format!("download of {repo} {file} failed: {why}"));
    let Some(root) = &cfg.download_dir else {
        return Err(fail("FAKE_WORKER_DOWNLOAD_DIR is not set".to_string()));
    };
    let parts: Vec<&str> = file.split('/').collect();
    if parts.iter().any(|p| p.is_empty() || *p == "." || *p == "..") {
        return Err(fail("unsafe file path".to_string()));
    }
    let source = parts.iter().fold(root.join(repo.replace('/', "__")), |acc, p| acc.join(p));
    if !source.is_file() {
        return Err(fail(format!("no fixture at {}", source.display())));
    }
    let dest = parts.iter().fold(PathBuf::from(local_dir), |acc, p| acc.join(p));
    let copied = dest
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|_| fs::copy(&source, &dest).map(|_| ()));
    match copied {
        Ok(()) => Ok(fields(vec![("path", json!(dest.display().to_string()))])),
        Err(e) => Err(fail(e.to_string())),
    }
}

fn dispatch(cfg: &Config, state: &mut State, op: &str, req: &Map<String, Value>) -> Reply {
    if !KNOWN_OPS.contains(&op) {
        return Err(failure("bad_request", format!("unknown op {op:?}")));
    }
    if op != "shutdown"
        && let Some((fail_op, code)) = &cfg.fail_op
        && fail_op == op
    {
        return Err(failure(code, "fake failure"));
    }
    match op {
        "hello" => op_hello(cfg),
        "probe" => op_probe(cfg),
        "load" => op_load(cfg, state, req),
        "voice" => op_voice(state, req),
        "synth" => op_synth(cfg, state, req),
        "kokoro_clip" => op_kokoro_clip(cfg, req),
        "download" => op_download(cfg, req),
        _ => Ok(Map::new()),
    }
}

/// Sleeps for `ms`, ending the worker with exit 0 as soon as stdin is closed.
fn interruptible_sleep(ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    loop {
        if STDIN_CLOSED.load(Ordering::SeqCst) {
            exit(0);
        }
        let now = Instant::now();
        if now >= end {
            return;
        }
        std::thread::sleep((end - now).min(Duration::from_millis(10)));
    }
}

fn error_reply(cfg: &Config, id: Value, err: &Failure) -> Value {
    json!({
        "id": id,
        "ok": false,
        "error": {
            "code": err.code,
            "message": err.message,
            "traceback": cfg.traceback,
        },
    })
}

fn handle_line(cfg: &Config, state: &mut State, pid: u32, line: &str) {
    let line = line.trim_end_matches(['\r', '\n']);
    let parsed: Option<Map<String, Value>> = match serde_json::from_str::<Value>(line) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    };
    let Some(req) = parsed else {
        let reply = error_reply(cfg, Value::Null, &failure("bad_request", "the line is not a JSON object"));
        send_line(&reply.to_string());
        return;
    };
    let id = match req.get("id") {
        Some(v) if v.is_number() => v.clone(),
        _ => Value::Null,
    };
    let op = req.get("op").and_then(Value::as_str).map(str::to_string);

    let mut entry = json!({ "event": "request", "pid": pid, "request": Value::Object(req.clone()) });
    if op.as_deref() == Some("voice") {
        let clip = req.get("clip").and_then(Value::as_str);
        let (exists, frames, rate) = clip.map_or((false, None, None), clip_facts);
        entry["clip_exists"] = json!(exists);
        entry["clip_frames"] = json!(frames);
        entry["clip_sample_rate"] = json!(rate);
    }
    append_log(cfg, &entry);

    let op_name = op.as_deref().unwrap_or("");
    if cfg.exit_op.as_deref() == Some(op_name) && !op_name.is_empty() {
        exit(3);
    }
    if op_name == "synth" && cfg.crash_chunk.is_some() && req.get("chunk").and_then(Value::as_u64) == cfg.crash_chunk {
        exit(3);
    }

    let outcome = match &op {
        Some(op) => dispatch(cfg, state, op, &req),
        None => Err(failure("bad_request", "the request has no string \"op\"")),
    };

    if op_name == "synth" && cfg.synth_delay_ms > 0 {
        interruptible_sleep(cfg.synth_delay_ms);
    }

    let mut reply_id = id;
    if cfg.wrong_id_op.as_deref() == Some(op_name) && !op_name.is_empty() {
        reply_id = json!(reply_id.as_i64().unwrap_or(0) + 7);
    }
    if cfg.garbage_op.as_deref() == Some(op_name) && !op_name.is_empty() {
        send_line("not json");
    } else {
        let reply = match outcome {
            Ok(mut f) => {
                let mut map = Map::new();
                map.insert("id".to_string(), reply_id);
                map.insert("ok".to_string(), Value::Bool(true));
                map.append(&mut f);
                Value::Object(map)
            }
            Err(e) => error_reply(cfg, reply_id, &e),
        };
        send_line(&reply.to_string());
    }
    if op_name == "shutdown" {
        exit(0);
    }
}

fn spawn_reader(tx: mpsc::Sender<Msg>) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(std::io::stdin().lock());
        let mut buf: Vec<u8> = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf).into_owned();
                    if tx.send(Msg::Line(line)).is_err() {
                        return;
                    }
                }
            }
        }
        STDIN_CLOSED.store(true, Ordering::SeqCst);
        let _ = tx.send(Msg::Eof);
    });
}

fn main() {
    let cfg = Config::from_env();
    let pid = std::process::id();
    let _ = writeln!(std::io::stderr(), "narrate-test-worker: started pid {pid}");
    append_log(&cfg, &json!({ "event": "start", "pid": pid }));
    write_env_dump(&cfg, pid);

    let (tx, rx) = mpsc::channel();
    spawn_reader(tx);
    let mut state = State::default();
    loop {
        match rx.recv() {
            Ok(Msg::Line(line)) => handle_line(&cfg, &mut state, pid, &line),
            Ok(Msg::Eof) | Err(_) => exit(0),
        }
    }
}
