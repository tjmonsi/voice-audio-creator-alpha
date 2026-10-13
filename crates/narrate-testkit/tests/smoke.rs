//! Smoke and contract tests for the test kit binaries (`narrate-test-worker`, `narrate-test-uv`,
//! `narrate-test-proxy`, `narrate-test-signal`). The integration test also makes Cargo build the
//! binaries during `cargo test --workspace` (spec-narrate-test-strategy.md, Requirement: Test kit).
//!
//! Sources: spec-narrate-test-strategy.md (Test kit table, Smoke test), spec-narrate-worker.md
//! (Requirement 9 and the Protocol schemas), spec-narrate-setup.md (Source allow-list, Child
//! process environment). This file is also the written contract of the knobs the `narrate`
//! black-box tests rely on; whoever implements the binaries must satisfy it.
//!
//! # `narrate-test-worker` (fake worker, spec-narrate-worker.md Requirement 9)
//!
//! Reads one JSON request per line on stdin, writes exactly one JSON reply per request on
//! stdout (replies in request order, `id` echoed), writes `narrate-test-worker: started pid
//! <pid>` to stderr at start, and exits 0 on stdin end-of-file (also while a request sleeps) and
//! right after replying to `shutdown`. Ops and reply fields follow the Protocol schemas. Knobs:
//!
//! | Variable | Meaning |
//! |---|---|
//! | `FAKE_WORKER_LOG` | Append one JSON line per event: `{"event":"start","pid":N}` at start and `{"event":"request","pid":N,"request":<parsed request>}` per request. A `voice` entry also carries `clip_exists` (bool), `clip_frames` (u64 or null) and `clip_sample_rate` (u32 or null), read from the RIFF headers of the clip at that moment |
//! | `FAKE_WORKER_ENV_DUMP` | File path (every `{pid}` replaced by the pid). At start write `{"pid":N,"cwd":"...","temp_exists":bool,"env":{NAME:value}}`; `temp_exists` tells whether the directory in `TEMP` exists |
//! | `FAKE_WORKER_DEVICE` | `cuda` (default), `cpu-fallback`, `cpu`, `cuda-unavailable`; fixes the `load` reply whatever device was requested (`cuda`: device cuda, name `FAKE_WORKER_DEVICE_NAME`, fallback false; `cpu-fallback`: cpu, null, true; `cpu`: cpu, null, false; `cuda-unavailable`: error `cuda_unavailable`) |
//! | `FAKE_WORKER_DEVICE_NAME` | Default `NVIDIA GeForce RTX 3050` |
//! | `FAKE_WORKER_PROBE_CUDA` | `1` (default): `probe` replies `cuda_available` true, `device_name` `Fake GPU`, `torch` `2.6.0+cu124`; `0`: `cuda_available` false, `device_name` null |
//! | `FAKE_WORKER_SAMPLES_PER_CHAR` | Default 100; `synth` returns `max(1, chars(text) x value)` samples |
//! | `FAKE_WORKER_SYNTH_SAMPLES` | When set, `synth` returns exactly this many samples |
//! | `FAKE_WORKER_CLIP_SAMPLES` | Default 240000; samples returned by `kokoro_clip` |
//! | `FAKE_WORKER_DOWNLOAD_DIR` | Fixtures at `<repo with / replaced by __>\<file with / replaced by \>`; `download` copies the fixture to `local_dir` joined with `file` (creating parents) and replies that path; a missing fixture gives `download_failed` with a message containing the file name |
//! | `FAKE_WORKER_DOWNLOAD_PATH_OVERRIDE` | `download` replies this path and writes nothing |
//! | `FAKE_WORKER_SYNTH_DELAY_MS` | Sleep before every `synth` reply |
//! | `FAKE_WORKER_PROTOCOL` | Protocol number reported by `hello` (default 1) |
//! | `FAKE_WORKER_TRACEBACK` | Text placed in `error.traceback` of every error reply (null when unset) |
//! | `FAKE_WORKER_FAIL_OP` | `<op>:<code>`: every request of that op replies `ok:false` with that code and message `fake failure` |
//! | `FAKE_WORKER_WRONG_ID_OP` | The reply to that op carries `id` = request id + 7 |
//! | `FAKE_WORKER_GARBAGE_OP` | The reply to that op is the non-JSON line `not json` |
//! | `FAKE_WORKER_EXIT_OP` | On receiving that op the worker exits with code 3 without replying |
//!
//! Hooks (same as the real worker): `NARRATE_TEST_FAIL_CHUNK=N` makes `synth` for chunk N reply
//! `test_failure`; `NARRATE_TEST_CRASH_CHUNK=N` makes the worker exit with code 3 without a
//! reply on `synth` for chunk N; `NARRATE_TEST_WATERMARK_FAIL=1` makes `load` reply
//! `watermarker_unavailable`. `synth` replies carry `synth_seconds` 0.5. `voice` before `load`
//! and `synth` for an unprepared voice reply `bad_request`; an unknown op or a line that is not
//! a JSON object replies `bad_request` (id echoed, or null).
//!
//! # `narrate-test-uv` (fake uv)
//!
//! As in the spec table, plus `FAKE_UV_ENV_DUMP` (append `{"args":[...],"env":{...}}` per
//! invocation) and `FAKE_UV_SYNC_NO_PACKAGE=1` (a non-offline `sync` creates `pyvenv.cfg` and
//! `Scripts\python.exe` but not the package `METADATA`). `FAKE_UV_LOG` lines are the arguments
//! joined by single spaces. A copy named `uv.exe` must work exactly like `narrate-test-uv.exe`.
//!
//! # `narrate-test-signal`
//!
//! `<process-group-id>` sends CTRL_BREAK_EVENT (exit 0 on success, 1 on failure); `alive <pid>`
//! exits 0 while the process exists and has not exited, else 1; `spawn-group <exe> [args...]`
//! starts the program with `CREATE_NEW_PROCESS_GROUP` (stdio inherited), prints its pid as the
//! first stdout line, waits for it and exits with its exit code. `spawn-group` exists because
//! PowerShell `Start-Process` cannot set that flag; QA scripts and instructions use it to start
//! `narrate` before sending `narrate-test-signal <pid>`.
//!
//! # `narrate-test-proxy`
//!
//! `--listen 127.0.0.1:<port> --log <file>`; see the tests below.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const WORKER: &str = env!("CARGO_BIN_EXE_narrate-test-worker");
const UV: &str = env!("CARGO_BIN_EXE_narrate-test-uv");
const PROXY: &str = env!("CARGO_BIN_EXE_narrate-test-proxy");
const SIGNAL: &str = env!("CARGO_BIN_EXE_narrate-test-signal");

const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

// ---------------------------------------------------------------------------------------
// Helpers (the test kit may depend only on serde, serde_json and windows-sys)
// ---------------------------------------------------------------------------------------

struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Tmp {
        static N: AtomicUsize = AtomicUsize::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "narrate-testkit-{}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst),
            stamp
        ));
        std::fs::create_dir_all(&p).expect("create temp dir");
        Tmp(p)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn clean(cmd: &mut Command) {
    for (k, _) in std::env::vars_os() {
        let name = k.to_string_lossy().to_ascii_uppercase();
        if name.starts_with("FAKE_") || name.starts_with("NARRATE_") {
            cmd.env_remove(&k);
        }
    }
}

struct Worker {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<String>,
    err_rx: Receiver<String>,
}

fn worker(envs: &[(&str, &str)]) -> Worker {
    let mut cmd = Command::new(WORKER);
    clean(&mut cmd);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("start narrate-test-worker");
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = channel::<String>();
    std::thread::spawn(move || {
        let mut r = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match r.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if tx.send(line.trim_end_matches(['\r', '\n']).to_string()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let (etx, err_rx) = channel::<String>();
    std::thread::spawn(move || {
        let mut r = BufReader::new(stderr);
        loop {
            let mut line = String::new();
            match r.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if etx.send(line.trim_end_matches(['\r', '\n']).to_string()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    Worker { child, stdin, rx, err_rx }
}

impl Worker {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn send_line(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        stdin.write_all(line.as_bytes()).expect("write request");
        stdin.write_all(b"\n").expect("write newline");
        stdin.flush().expect("flush");
    }

    fn recv_raw(&mut self) -> Option<String> {
        self.rx.recv_timeout(Duration::from_secs(30)).ok()
    }

    fn recv(&mut self) -> Value {
        let line = self.recv_raw().expect("the worker must reply within 30 s");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("reply is not JSON ({e}): {line}"))
    }

    fn call(&mut self, request: Value) -> Value {
        self.send_line(&request.to_string());
        self.recv()
    }

    fn close_stdin(&mut self) {
        self.stdin = None;
    }

    fn wait_exit(&mut self, limit: Duration) -> Option<i32> {
        let start = Instant::now();
        loop {
            if let Some(st) = self.child.try_wait().expect("try_wait") {
                return Some(st.code().unwrap_or(-1));
            }
            if start.elapsed() >= limit {
                return None;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn stderr_line(&mut self) -> Option<String> {
        self.err_rx.recv_timeout(Duration::from_secs(10)).ok()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn hello(w: &mut Worker, id: u64) -> Value {
    w.call(json!({"id": id, "op": "hello", "protocol": 1}))
}

fn load_and_voice(w: &mut Worker) {
    let r = w.call(json!({"id": 1, "op": "hello", "protocol": 1}));
    assert_eq!(r["ok"], true);
    let r = w.call(json!({"id": 2, "op": "load", "model_dir": "C:\\models", "device": "auto"}));
    assert_eq!(r["ok"], true, "{r}");
    let r = w.call(json!({"id": 3, "op": "voice", "voice_id": "builtin", "clip": Value::Null}));
    assert_eq!(r["ok"], true, "{r}");
}

fn decode_hex_f32(hex: &str) -> Vec<f32> {
    let bytes: Vec<u8> = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex digit pair"))
        .collect();
    bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

fn run_tool(exe: &str, args: &[&str], envs: &[(&str, &str)]) -> (i32, String, String) {
    let mut cmd = Command::new(exe);
    clean(&mut cmd);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.args(args).stdin(Stdio::null()).output().expect("run tool");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn tiny_wav(rate: u32, frames: u32) -> Vec<u8> {
    let data_len = frames * 2;
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    v.resize(v.len() + data_len as usize, 0);
    v
}

// ---------------------------------------------------------------------------------------
// narrate-test-worker
// ---------------------------------------------------------------------------------------

// [Scenario: Smoke test | spec-narrate-test-strategy | FR-005]
#[test]
fn worker_answers_hello_with_id_ok_and_protocol() {
    let mut w = worker(&[]);
    let r = w.call(json!({"id": 1, "op": "hello", "protocol": 1}));
    assert!(r.is_object());
    assert_eq!(r["id"], 1);
    assert_eq!(r["ok"], true);
    assert_eq!(r["protocol"], 1);
    assert!(r["worker"].is_string() && r["python"].is_string(), "{r}");
}

// [Scenario: Fake exits on stdin close | spec-narrate-worker Req 9 | FR-005]
#[test]
fn worker_exits_0_when_stdin_closes_without_shutdown() {
    let mut w = worker(&[]);
    assert_eq!(hello(&mut w, 1)["ok"], true);
    w.close_stdin();
    assert_eq!(w.wait_exit(Duration::from_secs(10)), Some(0));
}

// [Requirement 9: shutdown | spec-narrate-worker | FR-005]
#[test]
fn worker_replies_ok_to_shutdown_then_exits_0() {
    let mut w = worker(&[]);
    let r = w.call(json!({"id": 1, "op": "shutdown"}));
    assert_eq!(r["ok"], true);
    assert_eq!(r["id"], 1);
    assert_eq!(w.wait_exit(Duration::from_secs(10)), Some(0));
}

// [Requirement 9: stderr banner | spec-narrate-worker | FR-005]
#[test]
fn worker_announces_itself_on_stderr() {
    let mut w = worker(&[]);
    let line = w.stderr_line().expect("a stderr line at start");
    assert_eq!(line, format!("narrate-test-worker: started pid {}", w.pid()));
}

// [Scenario: Error response shape | spec-narrate-worker Req 3 | FR-005]
#[test]
fn unknown_op_gets_bad_request_with_the_id_echoed() {
    let mut w = worker(&[]);
    let r = w.call(json!({"id": 7, "op": "dance"}));
    assert_eq!(r["id"], 7);
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["code"], "bad_request");
    assert!(r["error"]["message"].is_string());
    assert!(r["error"]["traceback"].is_null());
}

// [Requirement 3: framing | spec-narrate-worker | FR-005]
#[test]
fn non_json_and_non_object_lines_get_bad_request_with_null_id() {
    let mut w = worker(&[]);
    for line in ["this is not json", "[1,2,3]", "42"] {
        w.send_line(line);
        let r = w.recv();
        assert_eq!(r["ok"], false, "{line}");
        assert_eq!(r["error"]["code"], "bad_request", "{line}");
        assert!(r["id"].is_null(), "{line}: {r}");
    }
    // Missing fields: no op.
    let r = w.call(json!({"id": 9}));
    assert_eq!(r["id"], 9);
    assert_eq!(r["error"]["code"], "bad_request");
}

// [Requirement 3: correlation and order | spec-narrate-worker | FR-005]
#[test]
fn replies_come_in_request_order_with_matching_ids() {
    let mut w = worker(&[]);
    for id in [1u64, 2, 3] {
        w.send_line(&json!({"id": id, "op": "hello", "protocol": 1}).to_string());
    }
    for id in [1u64, 2, 3] {
        assert_eq!(w.recv()["id"], id);
    }
}

// [Protocol: probe | spec-narrate-worker | FR-014]
#[test]
fn probe_defaults_and_cuda_off_knob() {
    let mut w = worker(&[]);
    let r = w.call(json!({"id": 1, "op": "probe"}));
    assert_eq!(r["ok"], true);
    assert_eq!(r["cuda_available"], true);
    assert_eq!(r["device_name"], "Fake GPU");
    assert_eq!(r["torch"], "2.6.0+cu124");

    let mut off = worker(&[("FAKE_WORKER_PROBE_CUDA", "0")]);
    let r = off.call(json!({"id": 1, "op": "probe"}));
    assert_eq!(r["cuda_available"], false);
    assert!(r["device_name"].is_null());
    assert_eq!(r["torch"], "2.6.0+cu124");
}

// [Protocol: load | spec-narrate-worker | FR-013]
#[test]
fn load_reply_is_fixed_by_the_device_knob() {
    let load = |envs: &[(&str, &str)]| {
        let mut w = worker(envs);
        w.call(json!({"id": 1, "op": "load", "model_dir": "C:\\models", "device": "auto"}))
    };
    let r = load(&[]);
    assert_eq!(
        (r["ok"].clone(), r["device"].clone(), r["fallback"].clone()),
        (json!(true), json!("cuda"), json!(false))
    );
    assert_eq!(r["device_name"], "NVIDIA GeForce RTX 3050");
    assert!(r["load_seconds"].is_number());

    let r = load(&[("FAKE_WORKER_DEVICE", "cuda"), ("FAKE_WORKER_DEVICE_NAME", "Fake GPU 9000")]);
    assert_eq!(r["device_name"], "Fake GPU 9000");

    let r = load(&[("FAKE_WORKER_DEVICE", "cpu-fallback")]);
    assert_eq!((r["device"].clone(), r["fallback"].clone()), (json!("cpu"), json!(true)));
    assert!(r["device_name"].is_null());

    let r = load(&[("FAKE_WORKER_DEVICE", "cpu")]);
    assert_eq!((r["device"].clone(), r["fallback"].clone()), (json!("cpu"), json!(false)));

    let r = load(&[("FAKE_WORKER_DEVICE", "cuda-unavailable")]);
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["code"], "cuda_unavailable");
}

// [Requirement 4 and 9: watermark hook | spec-narrate-worker | FR-018]
#[test]
fn watermark_hook_makes_load_reply_watermarker_unavailable() {
    let mut w = worker(&[("NARRATE_TEST_WATERMARK_FAIL", "1")]);
    let r = w.call(json!({"id": 1, "op": "load", "model_dir": "C:\\models", "device": "auto"}));
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["code"], "watermarker_unavailable");
}

// [Scenario: Reply shape | spec-narrate-worker Req 6 | FR-005]
#[test]
fn synth_reply_has_chunk_rate_samples_seconds_and_matching_hex() {
    let mut w = worker(&[]);
    load_and_voice(&mut w);
    let r = w.call(json!({"id": 4, "op": "synth", "chunk": 2, "total": 5, "voice_id": "builtin", "text": "abcde", "seed": 2}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["id"], 4);
    assert_eq!(r["chunk"], 2);
    assert_eq!(r["sample_rate"], 24000);
    let samples = r["samples"].as_u64().unwrap();
    assert_eq!(samples, 5 * 100, "default 100 samples per character");
    assert!(r["synth_seconds"].as_f64().unwrap() >= 0.0);
    let hex = r["pcm_f32le_hex"].as_str().unwrap();
    assert_eq!(hex.len() as u64, samples * 8);
    assert!(
        hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "lowercase hex only"
    );
    let floats = decode_hex_f32(hex);
    assert_eq!(floats.len() as u64, samples);
    assert!(floats.iter().all(|f| f.is_finite() && f.abs() <= 1.0));
}

// [Protocol: synth sample knobs | spec-narrate-worker | FR-005]
#[test]
fn synth_sample_count_knobs() {
    let mut w = worker(&[("FAKE_WORKER_SAMPLES_PER_CHAR", "3")]);
    load_and_voice(&mut w);
    let r = w.call(json!({"id": 4, "op": "synth", "chunk": 1, "total": 1, "voice_id": "builtin", "text": "héllo", "seed": 1}));
    assert_eq!(r["samples"], 15, "characters, not bytes: 5 x 3");

    let mut fixed = worker(&[("FAKE_WORKER_SYNTH_SAMPLES", "7")]);
    load_and_voice(&mut fixed);
    let r = fixed.call(json!({"id": 4, "op": "synth", "chunk": 1, "total": 1, "voice_id": "builtin", "text": "a much longer text than seven", "seed": 1}));
    assert_eq!(r["samples"], 7);
    assert_eq!(r["pcm_f32le_hex"].as_str().unwrap().len(), 56);

    let mut tiny = worker(&[("FAKE_WORKER_SAMPLES_PER_CHAR", "0")]);
    load_and_voice(&mut tiny);
    let r = tiny.call(json!({"id": 4, "op": "synth", "chunk": 1, "total": 1, "voice_id": "builtin", "text": "x", "seed": 1}));
    assert!(r["samples"].as_u64().unwrap() >= 1, "samples are at least 1");
}

// [Protocol: bad_request before load | spec-narrate-worker | FR-005]
#[test]
fn voice_before_load_and_synth_without_a_prepared_voice_are_bad_requests() {
    let mut w = worker(&[]);
    let r = w.call(json!({"id": 1, "op": "voice", "voice_id": "builtin", "clip": Value::Null}));
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["code"], "bad_request");
    let r = w.call(json!({"id": 2, "op": "load", "model_dir": "C:\\models", "device": "auto"}));
    assert_eq!(r["ok"], true);
    let r = w.call(json!({"id": 3, "op": "synth", "chunk": 1, "total": 1, "voice_id": "female", "text": "hi", "seed": 1}));
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["code"], "bad_request");
}

// [Requirement 6 and 9: failure hooks | spec-narrate-worker | FR-005]
#[test]
fn fail_chunk_hook_replies_test_failure_for_exactly_that_chunk() {
    let mut w = worker(&[("NARRATE_TEST_FAIL_CHUNK", "2")]);
    load_and_voice(&mut w);
    let ok = w.call(json!({"id": 4, "op": "synth", "chunk": 1, "total": 3, "voice_id": "builtin", "text": "one", "seed": 1}));
    assert_eq!(ok["ok"], true);
    let bad = w.call(json!({"id": 5, "op": "synth", "chunk": 2, "total": 3, "voice_id": "builtin", "text": "two", "seed": 2}));
    assert_eq!(bad["ok"], false);
    assert_eq!(bad["error"]["code"], "test_failure");
    let ok = w.call(json!({"id": 6, "op": "synth", "chunk": 3, "total": 3, "voice_id": "builtin", "text": "three", "seed": 3}));
    assert_eq!(ok["ok"], true);
}

// [Requirement 6 and 9: crash hook | spec-narrate-worker | FR-005]
#[test]
fn crash_chunk_hook_exits_with_code_3_without_replying() {
    let mut w = worker(&[("NARRATE_TEST_CRASH_CHUNK", "1")]);
    load_and_voice(&mut w);
    w.send_line(
        &json!({"id": 4, "op": "synth", "chunk": 1, "total": 1, "voice_id": "builtin", "text": "boom", "seed": 1})
            .to_string(),
    );
    assert_eq!(w.recv_raw(), None, "no reply is written");
    assert_eq!(w.wait_exit(Duration::from_secs(10)), Some(3));
}

// [Requirement 9: knobs for protocol faults | spec-narrate-worker | FR-005]
#[test]
fn protocol_fault_knobs() {
    let mut w = worker(&[("FAKE_WORKER_PROTOCOL", "2")]);
    assert_eq!(hello(&mut w, 1)["protocol"], 2);

    let mut w = worker(&[
        ("FAKE_WORKER_FAIL_OP", "probe:probe_failed"),
        ("FAKE_WORKER_TRACEBACK", "Traceback (most recent call last):\n  boom"),
    ]);
    let r = w.call(json!({"id": 1, "op": "probe"}));
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["code"], "probe_failed");
    assert_eq!(r["error"]["traceback"], "Traceback (most recent call last):\n  boom");
    assert_eq!(hello(&mut w, 2)["ok"], true, "other ops are unaffected");

    let mut w = worker(&[("FAKE_WORKER_WRONG_ID_OP", "hello")]);
    assert_eq!(hello(&mut w, 1)["id"], 8);

    let mut w = worker(&[("FAKE_WORKER_GARBAGE_OP", "hello")]);
    w.send_line(&json!({"id": 1, "op": "hello", "protocol": 1}).to_string());
    assert_eq!(w.recv_raw().as_deref(), Some("not json"));

    let mut w = worker(&[("FAKE_WORKER_EXIT_OP", "load")]);
    assert_eq!(hello(&mut w, 1)["ok"], true);
    w.send_line(
        &json!({"id": 2, "op": "load", "model_dir": "C:\\models", "device": "auto"}).to_string(),
    );
    assert_eq!(w.recv_raw(), None);
    assert_eq!(w.wait_exit(Duration::from_secs(10)), Some(3));
}

// [Protocol: synth delay knob | spec-narrate-worker | FR-005]
#[test]
fn synth_delay_knob_sleeps_before_replying_and_stdin_close_still_ends_the_worker() {
    let mut w = worker(&[("FAKE_WORKER_SYNTH_DELAY_MS", "400")]);
    load_and_voice(&mut w);
    let start = Instant::now();
    let r = w.call(json!({"id": 4, "op": "synth", "chunk": 1, "total": 1, "voice_id": "builtin", "text": "slow", "seed": 1}));
    assert_eq!(r["ok"], true);
    assert!(start.elapsed() >= Duration::from_millis(350), "{:?}", start.elapsed());

    let mut w = worker(&[("FAKE_WORKER_SYNTH_DELAY_MS", "30000")]);
    load_and_voice(&mut w);
    w.send_line(
        &json!({"id": 4, "op": "synth", "chunk": 1, "total": 1, "voice_id": "builtin", "text": "stuck", "seed": 1})
            .to_string(),
    );
    std::thread::sleep(Duration::from_millis(200));
    w.close_stdin();
    assert_eq!(
        w.wait_exit(Duration::from_secs(10)),
        Some(0),
        "EOF ends the worker even while a request runs"
    );
}

// [Protocol: download | spec-narrate-worker Req 8 and 9 | FR-010]
#[test]
fn download_copies_the_fixture_into_local_dir_and_replies_its_path() {
    let t = Tmp::new();
    let fixtures = t.join("fixtures");
    std::fs::create_dir_all(fixtures.join("ResembleAI__chatterbox-turbo")).unwrap();
    std::fs::create_dir_all(fixtures.join("hexgrad__Kokoro-82M").join("voices")).unwrap();
    std::fs::write(fixtures.join("ResembleAI__chatterbox-turbo").join("conds.pt"), b"conds bytes")
        .unwrap();
    std::fs::write(
        fixtures.join("hexgrad__Kokoro-82M").join("voices").join("af_heart.pt"),
        b"voice bytes",
    )
    .unwrap();
    let local = t.join("staging").join("models");
    let mut w = worker(&[("FAKE_WORKER_DOWNLOAD_DIR", fixtures.to_str().unwrap())]);

    let r = w.call(json!({"id": 1, "op": "download", "repo": "ResembleAI/chatterbox-turbo",
        "revision": "749d1c1a46eb10492095d68fbcf55691ccf137cd", "file": "conds.pt", "local_dir": local.to_str().unwrap()}));
    assert_eq!(r["ok"], true, "{r}");
    let reply_path = PathBuf::from(r["path"].as_str().unwrap());
    assert_eq!(std::fs::read(&reply_path).unwrap(), b"conds bytes");
    assert_eq!(
        reply_path.to_string_lossy().to_ascii_lowercase(),
        local.join("conds.pt").to_string_lossy().to_ascii_lowercase()
    );

    let r = w.call(json!({"id": 2, "op": "download", "repo": "hexgrad/Kokoro-82M",
        "revision": "f3ff3571791e39611d31c381e3a41a3af07b4987", "file": "voices/af_heart.pt", "local_dir": local.to_str().unwrap()}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(
        std::fs::read(local.join("voices").join("af_heart.pt")).unwrap(),
        b"voice bytes",
        "parent directories are created"
    );

    let r = w.call(json!({"id": 3, "op": "download", "repo": "ResembleAI/chatterbox-turbo",
        "revision": "749d1c1a46eb10492095d68fbcf55691ccf137cd", "file": "vocab.json", "local_dir": local.to_str().unwrap()}));
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["code"], "download_failed");
    assert!(r["error"]["message"].as_str().unwrap().contains("vocab.json"));

    let mut over = worker(&[("FAKE_WORKER_DOWNLOAD_PATH_OVERRIDE", "C:\\somewhere\\else.bin")]);
    let r = over.call(json!({"id": 1, "op": "download", "repo": "ResembleAI/chatterbox-turbo",
        "revision": "749d1c1a46eb10492095d68fbcf55691ccf137cd", "file": "conds.pt", "local_dir": local.to_str().unwrap()}));
    assert_eq!(r["path"], "C:\\somewhere\\else.bin");
}

// [Protocol: kokoro_clip | spec-narrate-worker | FR-010]
#[test]
fn kokoro_clip_returns_24k_float_samples_with_the_configured_length() {
    let mut w = worker(&[("FAKE_WORKER_CLIP_SAMPLES", "10")]);
    let r = w.call(json!({"id": 1, "op": "kokoro_clip", "model_dir": "C:\\models\\kokoro", "voice": "af_heart", "text": "hello", "device": "auto"}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["samples"], 10);
    assert_eq!(r["sample_rate"], 24000);
    assert!(r["device"] == "cuda" || r["device"] == "cpu");
    assert_eq!(r["pcm_f32le_hex"].as_str().unwrap().len(), 80);

    let mut d = worker(&[]);
    let r = d.call(json!({"id": 1, "op": "kokoro_clip", "model_dir": "C:\\models\\kokoro", "voice": "am_michael", "text": "hello", "device": "cpu"}));
    assert_eq!(r["samples"], 240_000, "default clip length");
    assert_eq!(r["pcm_f32le_hex"].as_str().unwrap().len(), 240_000 * 8);
}

// [Requirement 9: logging and environment dump knobs | spec-narrate-worker | NFR-002]
#[test]
fn log_and_environment_dump_knobs() {
    let t = Tmp::new();
    let log = t.join("worker.log");
    let dump_template = t.join("env-{pid}.json");
    let clip = t.join("clip.wav");
    std::fs::write(&clip, tiny_wav(24_000, 100)).unwrap();
    let temp_dir = t.join("temp-dir");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mut w = worker(&[
        ("FAKE_WORKER_LOG", log.to_str().unwrap()),
        ("FAKE_WORKER_ENV_DUMP", dump_template.to_str().unwrap()),
        ("TEMP", temp_dir.to_str().unwrap()),
    ]);
    let pid = w.pid() as u64;
    assert_eq!(hello(&mut w, 1)["ok"], true);
    w.call(json!({"id": 2, "op": "load", "model_dir": "C:\\m", "device": "auto"}));
    let r = w.call(json!({"id": 3, "op": "voice", "voice_id": "file", "clip": clip.to_str().unwrap()}));
    assert_eq!(r["ok"], true, "{r}");
    let r = w.call(json!({"id": 4, "op": "voice", "voice_id": "gone", "clip": t.join("missing.wav").to_str().unwrap()}));
    assert_eq!(r["ok"], true, "a clip that does not exist is only reported in the log: {r}");
    w.call(json!({"id": 5, "op": "shutdown"}));
    assert_eq!(w.wait_exit(Duration::from_secs(10)), Some(0));

    let entries: Vec<Value> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    assert_eq!(entries[0]["event"], "start");
    assert_eq!(entries[0]["pid"], pid);
    let requests: Vec<&Value> = entries.iter().filter(|e| e["event"] == "request").collect();
    let ops: Vec<&str> = requests.iter().map(|e| e["request"]["op"].as_str().unwrap()).collect();
    assert_eq!(ops, vec!["hello", "load", "voice", "voice", "shutdown"]);
    assert!(requests.iter().all(|e| e["pid"] == pid));
    assert_eq!(requests[2]["clip_exists"], true);
    assert_eq!(requests[2]["clip_frames"], 100);
    assert_eq!(requests[2]["clip_sample_rate"], 24000);
    assert_eq!(requests[3]["clip_exists"], false);
    assert!(requests[3]["clip_frames"].is_null());

    let dump_path = t.join(&format!("env-{pid}.json"));
    let dump: Value = serde_json::from_str(&std::fs::read_to_string(dump_path).unwrap()).unwrap();
    assert_eq!(dump["pid"], pid);
    assert!(dump["cwd"].is_string());
    assert_eq!(dump["temp_exists"], true);
    assert!(dump["env"].is_object());
    let env = dump["env"].as_object().unwrap();
    assert!(env
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("TEMP") && v.as_str() == temp_dir.to_str()));
}

// ---------------------------------------------------------------------------------------
// narrate-test-uv
// ---------------------------------------------------------------------------------------

// [Test kit: fake uv --version | spec-narrate-test-strategy | FR-010 | FR-011]
#[test]
fn uv_version_defaults_to_0_10_2_and_follows_the_variable() {
    let (code, out, _) = run_tool(UV, &["--version"], &[]);
    assert_eq!((code, out.trim_end()), (0, "uv 0.10.2 (test)"));
    let (code, out, _) = run_tool(UV, &["--version"], &[("FAKE_UV_VERSION", "0.13.0")]);
    assert_eq!((code, out.trim_end()), (0, "uv 0.13.0 (test)"));
}

// [Test kit: fake uv log | spec-narrate-test-strategy | FR-010]
#[test]
fn uv_appends_its_arguments_as_one_line_per_invocation() {
    let t = Tmp::new();
    let log = t.join("uv.log");
    let envs = [("FAKE_UV_LOG", log.to_str().unwrap())];
    run_tool(UV, &["--version"], &envs);
    run_tool(UV, &["python", "install", "3.11.14", "--no-bin", "--no-registry", "--offline"], &envs);
    run_tool(UV, &["frobnicate"], &envs);
    let text = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        vec!["--version", "python install 3.11.14 --no-bin --no-registry --offline", "frobnicate"]
    );
}

// [Test kit: fake uv python install | spec-narrate-test-strategy | FR-010 | NFR-001]
#[test]
fn uv_python_install_needs_the_cached_archive_offline_and_creates_it_online() {
    let t = Tmp::new();
    let archive = t.join("archive.tar.gz");
    std::fs::write(&archive, b"archive bytes").unwrap();
    let cache = t.join("python-cache");
    let install = t.join("python");
    let envs = [
        ("FAKE_UV_PY_ARCHIVE", archive.to_str().unwrap()),
        ("UV_PYTHON_CACHE_DIR", cache.to_str().unwrap()),
        ("UV_PYTHON_INSTALL_DIR", install.to_str().unwrap()),
    ];
    let offline = ["python", "install", "3.11.14", "--no-bin", "--no-registry", "--offline"];
    let online = ["python", "install", "3.11.14", "--no-bin", "--no-registry"];
    assert_eq!(run_tool(UV, &offline, &envs).0, 1, "offline without a cached copy fails");
    assert_eq!(run_tool(UV, &online, &envs).0, 0);
    assert!(cache.is_dir() && install.is_dir());
    assert_eq!(std::fs::read(cache.join("archive.tar.gz")).unwrap(), b"archive bytes");
    assert_eq!(run_tool(UV, &offline, &envs).0, 0, "offline succeeds once the archive is cached");
}

// [Test kit: fake uv sync | spec-narrate-test-strategy | FR-010]
#[test]
fn uv_sync_creates_the_marker_files_online_and_requires_them_offline() {
    let t = Tmp::new();
    let venv = t.join("venv");
    let envs = [("UV_PROJECT_ENVIRONMENT", venv.to_str().unwrap())];
    let offline = ["sync", "--project", "x", "--frozen", "--offline"];
    let online = ["sync", "--project", "x", "--frozen"];
    assert_eq!(run_tool(UV, &offline, &envs).0, 1);
    assert_eq!(run_tool(UV, &online, &envs).0, 0);
    for rel in [
        "pyvenv.cfg",
        "Scripts\\python.exe",
        "Lib\\site-packages\\chatterbox_tts-0.1.7.dist-info\\METADATA",
    ] {
        let p = venv.join(rel);
        assert!(p.is_file(), "{rel}");
        assert_eq!(std::fs::metadata(&p).unwrap().len(), 0, "{rel} is empty");
    }
    assert_eq!(run_tool(UV, &offline, &envs).0, 0);
}

// [Test kit: fake uv sync knobs | spec-narrate-test-strategy | FR-010 | NFR-001]
#[test]
fn uv_sync_failure_sleep_and_no_package_knobs() {
    let t = Tmp::new();
    let venv = t.join("venv");
    let venv_s = venv.to_str().unwrap();
    for args in [&["sync", "--offline"][..], &["sync"][..]] {
        let (code, _, err) = run_tool(
            UV,
            args,
            &[
                ("UV_PROJECT_ENVIRONMENT", venv_s),
                ("FAKE_UV_SYNC_EXIT", "2"),
                ("FAKE_UV_SYNC_STDERR", "error: boom"),
            ],
        );
        assert_eq!(code, 2, "{args:?}");
        assert!(err.contains("error: boom"), "{err}");
    }
    assert!(!venv.exists(), "a failing sync creates nothing");

    let start = Instant::now();
    run_tool(
        UV,
        &["sync"],
        &[("UV_PROJECT_ENVIRONMENT", venv_s), ("FAKE_UV_SYNC_SLEEP_MS", "400")],
    );
    assert!(start.elapsed() >= Duration::from_millis(350), "{:?}", start.elapsed());

    let venv2 = t.join("venv2");
    let (code, _, _) = run_tool(
        UV,
        &["sync"],
        &[("UV_PROJECT_ENVIRONMENT", venv2.to_str().unwrap()), ("FAKE_UV_SYNC_NO_PACKAGE", "1")],
    );
    assert_eq!(code, 0);
    assert!(venv2.join("pyvenv.cfg").is_file() && venv2.join("Scripts").join("python.exe").is_file());
    assert!(!venv2
        .join("Lib")
        .join("site-packages")
        .join("chatterbox_tts-0.1.7.dist-info")
        .join("METADATA")
        .exists());
}

// [Test kit: fake uv other invocations | spec-narrate-test-strategy | FR-010]
#[test]
fn uv_other_invocations_exit_with_fake_uv_exit() {
    assert_eq!(run_tool(UV, &["frobnicate"], &[]).0, 1);
    assert_eq!(run_tool(UV, &["frobnicate"], &[("FAKE_UV_EXIT", "7")]).0, 7);
}

// [Test kit: fake uv environment dump | spec-narrate-test-strategy | NFR-002]
#[test]
fn uv_environment_dump_appends_one_json_object_per_invocation() {
    let t = Tmp::new();
    let dump = t.join("uvenv.jsonl");
    let envs = [("FAKE_UV_ENV_DUMP", dump.to_str().unwrap()), ("UV_CACHE_DIR", "C:\\somewhere")];
    run_tool(UV, &["--version"], &envs);
    run_tool(UV, &["sync", "--offline"], &envs);
    let entries: Vec<Value> = std::fs::read_to_string(&dump)
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["args"], json!(["--version"]));
    assert_eq!(entries[1]["args"], json!(["sync", "--offline"]));
    let env = entries[0]["env"].as_object().unwrap();
    assert!(env.iter().any(|(k, v)| k == "UV_CACHE_DIR" && v == "C:\\somewhere"));
}

// [Test kit: uv.exe copy | spec-narrate-setup Preconditions | FR-010]
#[test]
fn a_copy_named_uv_exe_behaves_like_the_fake_uv() {
    let t = Tmp::new();
    let copy = t.join("uv.exe");
    std::fs::copy(UV, &copy).unwrap();
    let (code, out, _) = run_tool(copy.to_str().unwrap(), &["--version"], &[]);
    assert_eq!((code, out.trim_end()), (0, "uv 0.10.2 (test)"));
}

// ---------------------------------------------------------------------------------------
// narrate-test-signal
// ---------------------------------------------------------------------------------------

fn alive(pid: u64) -> bool {
    run_tool(SIGNAL, &["alive", &pid.to_string()], &[]).0 == 0
}

fn wait_for(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    loop {
        if f() {
            return true;
        }
        if start.elapsed() >= limit {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

// [Test kit: alive | spec-narrate-test-strategy | FR-005]
#[test]
fn alive_reports_running_processes_and_gone_processes() {
    assert!(alive(std::process::id() as u64), "this test process is running");
    let mut w = worker(&[]);
    let pid = w.pid() as u64;
    assert!(alive(pid));
    let _ = w.child.kill();
    let _ = w.child.wait();
    assert!(wait_for(Duration::from_secs(10), || !alive(pid)), "a terminated process is reported gone");
}

// [Test kit: spawn-group | clarification of spec-narrate-test-strategy QA item 10 | FR-005]
#[test]
fn spawn_group_prints_the_child_pid_first_and_exits_with_the_child_exit_code() {
    // The child is the signal helper itself: `alive <pid>` exits 0 for this process and 1 for a
    // pid that cannot exist.
    let me = std::process::id().to_string();
    let mut cmd = Command::new(SIGNAL);
    cmd.args(["spawn-group", SIGNAL, "alive", &me])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let out = cmd.output().expect("run spawn-group");
    assert_eq!(out.status.code(), Some(0), "exit code of the child is propagated");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let first = stdout.lines().next().expect("the pid is the first stdout line");
    assert!(first.trim().parse::<u32>().is_ok(), "first line is a pid: {first:?}");

    let out = Command::new(SIGNAL)
        .args(["spawn-group", SIGNAL, "alive", "4294967290"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .expect("run spawn-group");
    assert_eq!(out.status.code(), Some(1), "a failing child's exit code is propagated");
    assert!(String::from_utf8_lossy(&out.stdout).lines().next().unwrap().trim().parse::<u32>().is_ok());
}

struct Helper {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<String>,
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_group_worker() -> Helper {
    let mut cmd = Command::new(SIGNAL);
    clean(&mut cmd);
    cmd.args(["spawn-group", WORKER])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().expect("start spawn-group");
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = channel::<String>();
    std::thread::spawn(move || {
        let mut r = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match r.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let _ = tx.send(line.trim_end().to_string());
                }
            }
        }
    });
    Helper { child, stdin, rx }
}

// [Test kit: spawn-group child lifetime | clarification of QA item 10 | FR-005]
#[test]
fn spawn_group_child_runs_until_its_stdin_closes_and_its_exit_code_is_returned() {
    let mut h = spawn_group_worker();
    let first = h.rx.recv_timeout(Duration::from_secs(20)).expect("pid line");
    let pid: u64 = first.trim().parse().expect("first line is the pid of the spawned program");
    assert!(alive(pid), "the spawned program is running");
    drop(h.stdin.take()); // the worker sees end-of-file on the inherited stdin and exits 0
    let start = Instant::now();
    let status = loop {
        if let Some(st) = h.child.try_wait().unwrap() {
            break st;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "spawn-group must exit once the child exits"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(0));
    assert!(!alive(pid));
}

// [Test kit: CTRL_BREAK to a group leader | spec-narrate-test-strategy | FR-005]
#[test]
fn ctrl_break_ends_a_process_started_as_a_group_leader() {
    let mut cmd = Command::new(WORKER);
    clean(&mut cmd);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NEW_PROCESS_GROUP);
    let mut child = cmd.spawn().expect("start worker in a new process group");
    let _keep_stdin_open = child.stdin.take();
    let pid = child.id();
    let (code, _, _) = run_tool(SIGNAL, &[&pid.to_string()], &[]);
    assert_eq!(
        code, 0,
        "narrate-test-signal could not deliver CTRL_BREAK_EVENT; run the tests from a console session"
    );
    assert!(
        wait_for(Duration::from_secs(10), || child.try_wait().unwrap().is_some()),
        "the group leader must end"
    );
    let _ = child.kill();
}

// [Test kit: CTRL_BREAK through spawn-group | clarification of QA item 10 | FR-005]
#[test]
fn ctrl_break_reaches_a_program_started_through_spawn_group() {
    let mut h = spawn_group_worker();
    let first = h.rx.recv_timeout(Duration::from_secs(20)).expect("pid line");
    let pid: u32 = first.trim().parse().expect("pid");
    let (code, _, _) = run_tool(SIGNAL, &[&pid.to_string()], &[]);
    assert_eq!(
        code, 0,
        "narrate-test-signal could not deliver CTRL_BREAK_EVENT; run the tests from a console session"
    );
    let start = Instant::now();
    let status = loop {
        if let Some(st) = h.child.try_wait().unwrap() {
            break st;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the spawned program must be a group leader that ends on CTRL_BREAK"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_ne!(status.code(), Some(0), "the child was ended by the console event, not by end-of-file");
    assert!(wait_for(Duration::from_secs(10), || !alive(pid as u64)));
}

// ---------------------------------------------------------------------------------------
// narrate-test-proxy
// ---------------------------------------------------------------------------------------

struct Proxy {
    child: Child,
    port: u16,
    log: PathBuf,
    _dir: Tmp,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_proxy() -> Proxy {
    let dir = Tmp::new();
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").expect("free port");
        l.local_addr().unwrap().port()
    };
    let log = dir.join("proxy.log");
    let mut cmd = Command::new(PROXY);
    clean(&mut cmd);
    cmd.arg("--listen").arg(format!("127.0.0.1:{port}")).arg("--log").arg(&log);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let child = cmd.spawn().expect("start narrate-test-proxy");
    let ready = wait_for(Duration::from_secs(15), || TcpStream::connect(("127.0.0.1", port)).is_ok());
    assert!(ready, "the proxy must listen on 127.0.0.1:{port}");
    Proxy { child, port, log, _dir: dir }
}

impl Proxy {
    /// Sends a raw request and returns the first response line (empty when the connection is
    /// closed or silent).
    fn request(&self, raw: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).expect("connect to the proxy");
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut head = Vec::new();
        let mut buf = [0u8; 512];
        loop {
            match s.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    head.extend_from_slice(&buf[..n]);
                    if head.windows(2).any(|w| w == b"\r\n") {
                        break;
                    }
                }
            }
        }
        String::from_utf8_lossy(&head).lines().next().unwrap_or("").to_string()
    }

    fn connect(&self, target: &str) -> String {
        self.request(&format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n"))
    }

    fn log_lines(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|l| l.to_string())
            .collect()
    }

    fn has_log(&self, verdict: &str, target: &str) -> bool {
        self.log_lines().iter().any(|l| {
            let parts: Vec<&str> = l.split('\t').collect();
            parts.len() == 3
                && !parts[0].is_empty()
                && parts[0].bytes().all(|b| b.is_ascii_digit())
                && parts[1] == verdict
                && parts[2] == target
        })
    }
}

// [Test kit: allow-list proxy denies | spec-narrate-setup Source allow-list | FR-011 | BR-003]
#[test]
fn proxy_answers_403_and_logs_deny_for_hosts_outside_the_allow_list() {
    let p = start_proxy();
    for target in [
        "evil.example.com:443",
        "hf.co:443",
        "nothf.co:443",
        "evilhuggingface.co:443",
        "huggingface.co.evil.com:443",
        "example.org:443",
    ] {
        let first = p.connect(target);
        assert!(first.contains(" 403"), "{target}: {first:?}");
        assert!(
            wait_for(Duration::from_secs(5), || p.has_log("DENY", target)),
            "{target}: {:?}",
            p.log_lines()
        );
    }
}

// [Test kit: allow-list proxy rejects non-CONNECT | spec-narrate-test-strategy | FR-011]
#[test]
fn proxy_rejects_non_connect_requests_with_403() {
    let p = start_proxy();
    let first = p.request("GET http://huggingface.co/ HTTP/1.1\r\nHost: huggingface.co\r\n\r\n");
    assert!(first.contains(" 403"), "{first:?}");
}

// [Test kit: allow-list proxy allows | spec-narrate-setup Source allow-list | FR-011 | BR-003]
#[test]
fn proxy_logs_allow_and_does_not_answer_403_for_the_allow_listed_hosts_and_suffixes() {
    let p = start_proxy();
    // The proxy may be unable to reach the real host (no network): it then answers 502 or closes
    // the connection, which is fine; only the decision and the absence of a 403 matter.
    for target in [
        "pypi.org:443",
        "files.pythonhosted.org:443",
        "download.pytorch.org:443",
        "github.com:443",
        "objects.githubusercontent.com:443",
        "release-assets.githubusercontent.com:443",
        "huggingface.co:443",
        "cdn-lfs.hf.co:443",
        "cas-bridge.xethub.hf.co:443",
        "cdn-lfs.huggingface.co:443",
    ] {
        let first = p.connect(target);
        assert!(!first.contains(" 403"), "{target} must not be refused: {first:?}");
        assert!(
            wait_for(Duration::from_secs(20), || p.has_log("ALLOW", target)),
            "{target}: {:?}",
            p.log_lines()
        );
    }
}
