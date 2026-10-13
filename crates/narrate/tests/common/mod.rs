//! Shared helpers for the `narrate` black-box tests (binary paths, fabricated data
//! directory, pin id lookup, worker-log readers, WAV builders, SHA-256).
//!
//! Sources: specs/spec-narrate-test-strategy.md (Requirements: Test command and
//! dev-dependencies, Test kit, Fabricated ready data directory, Automated setup-step tests) and
//! specs/spec-narrate-worker.md (Requirement 9). No implementation was read.
//!
//! The test-kit binaries live next to `env!("CARGO_BIN_EXE_narrate")` (Cargo builds them for
//! `cargo test --workspace --all-features`). The knobs of `narrate-test-worker`,
//! `narrate-test-uv` and `narrate-test-signal` that these tests rely on are listed in the module
//! documentation of `crates/narrate-testkit/tests/smoke.rs`, which also tests them; the tests of
//! this crate use these in particular:
//!
//! * `FAKE_WORKER_LOG` (JSON lines: `start` and `request` events with the worker `pid`; `voice`
//!   entries add `clip_exists`, `clip_frames`, `clip_sample_rate`), `FAKE_WORKER_ENV_DUMP`
//!   (`{pid}` in the path is replaced; JSON `pid`, `cwd`, `temp_exists`, `env`),
//!   `FAKE_WORKER_DEVICE`, `FAKE_WORKER_DEVICE_NAME`, `FAKE_WORKER_PROBE_CUDA`,
//!   `FAKE_WORKER_SAMPLES_PER_CHAR`, `FAKE_WORKER_SYNTH_SAMPLES`, `FAKE_WORKER_CLIP_SAMPLES`,
//!   `FAKE_WORKER_DOWNLOAD_DIR`, `FAKE_WORKER_DOWNLOAD_PATH_OVERRIDE`,
//!   `FAKE_WORKER_SYNTH_DELAY_MS`, `FAKE_WORKER_PROTOCOL`, `FAKE_WORKER_TRACEBACK`,
//!   `FAKE_WORKER_FAIL_OP` (`<op>:<code>`), `FAKE_WORKER_WRONG_ID_OP`, `FAKE_WORKER_GARBAGE_OP`,
//!   `FAKE_WORKER_EXIT_OP`.
//! * The fake worker writes `narrate-test-worker: started pid <pid>` to stderr at start and
//!   replies `synth_seconds` 0.5 for every `synth`.
//! * `FAKE_UV_LOG`, `FAKE_UV_VERSION`, `FAKE_UV_PY_ARCHIVE`, `FAKE_UV_SYNC_SLEEP_MS`,
//!   `FAKE_UV_SYNC_EXIT`, `FAKE_UV_SYNC_STDERR` (spec table) plus `FAKE_UV_ENV_DUMP` and
//!   `FAKE_UV_SYNC_NO_PACKAGE`.

#![allow(dead_code)]

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

pub use tempfile::TempDir;

use serde_json::Value;

// ---------------------------------------------------------------------------------------
// Constants taken from the specs
// ---------------------------------------------------------------------------------------

pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Reference text of spec-narrate-setup.md (223 characters, three sentences).
pub const REFERENCE_TEXT: &str = "The morning light settled over the quiet harbor, and the fishing boats rocked gently against the pier. A gull called out twice, then glided low across the water. It will be a calm, clear day for a long walk along the coast.";

pub const TURBO_FILES: [&str; 9] = [
    "t3_turbo_v1.safetensors",
    "s3gen_meanflow.safetensors",
    "ve.safetensors",
    "conds.pt",
    "vocab.json",
    "merges.txt",
    "tokenizer_config.json",
    "special_tokens_map.json",
    "added_tokens.json",
];

pub const KOKORO_FILES: [&str; 4] =
    ["kokoro-v1_0.pth", "config.json", "voices/af_heart.pt", "voices/am_michael.pt"];

pub const TURBO_REPO: &str = "ResembleAI/chatterbox-turbo";
pub const KOKORO_REPO: &str = "hexgrad/Kokoro-82M";

// ---------------------------------------------------------------------------------------
// Binary locations
// ---------------------------------------------------------------------------------------

pub fn narrate_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_narrate"))
}

/// Locates a test-kit binary as a sibling of the `narrate` executable. Panics with a message
/// naming `cargo test --workspace --all-features` when it was not built.
pub fn kit_bin(name: &str) -> PathBuf {
    let exe = narrate_exe();
    let dir = exe.parent().expect("the narrate executable has a parent directory");
    let candidate = dir.join(format!("{name}.exe"));
    if !candidate.is_file() {
        panic!(
            "test-kit binary {name}.exe was not found at {}; run the tests with `cargo test --workspace --all-features` so that the narrate-testkit binaries are built",
            candidate.display()
        );
    }
    candidate
}

pub fn worker_exe() -> PathBuf {
    kit_bin("narrate-test-worker")
}

pub fn uv_exe() -> PathBuf {
    kit_bin("narrate-test-uv")
}

pub fn signal_exe() -> PathBuf {
    kit_bin("narrate-test-signal")
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

pub fn abs(p: &Path) -> PathBuf {
    std::path::absolute(p).expect("std::path::absolute")
}

// ---------------------------------------------------------------------------------------
// Running narrate
// ---------------------------------------------------------------------------------------

pub struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Out {
    pub fn stderr_lines(&self) -> Vec<&str> {
        self.stderr.lines().collect()
    }

    pub fn stdout_lines(&self) -> Vec<&str> {
        self.stdout.lines().collect()
    }

    pub fn has_stderr_line(&self, line: &str) -> bool {
        self.stderr.lines().any(|l| l == line)
    }

    pub fn count_stderr_line(&self, line: &str) -> usize {
        self.stderr.lines().filter(|l| *l == line).count()
    }

    pub fn find_stderr_prefix(&self, prefix: &str) -> Option<&str> {
        self.stderr.lines().find(|l| l.starts_with(prefix))
    }

    pub fn stderr_line_index(&self, line: &str) -> Option<usize> {
        self.stderr.lines().position(|l| l == line)
    }

    pub fn context(&self) -> String {
        format!(
            "exit code {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.code, self.stdout, self.stderr
        )
    }

    pub fn assert_code(&self, expected: i32) {
        assert_eq!(self.code, expected, "unexpected exit code\n{}", self.context());
    }

    /// Asserts the exit code, the exact `error[<code>]: <cause>` line and the `  fix: <fix>`
    /// line directly after it (NFR-007, NFR-008).
    pub fn assert_error(&self, exit: i32, code: &str, cause: &str, fix: &str) {
        self.assert_code(exit);
        let want = format!("error[{code}]: {cause}");
        let lines: Vec<&str> = self.stderr.lines().collect();
        let idx = lines
            .iter()
            .position(|l| *l == want)
            .unwrap_or_else(|| panic!("missing stderr line {want:?}\n{}", self.context()));
        let fix_line = format!("  fix: {fix}");
        assert_eq!(
            lines.get(idx + 1).map(|l| l.to_string()),
            Some(fix_line),
            "the fix line must directly follow the error line\n{}",
            self.context()
        );
    }

    /// Like `assert_error` for causes that contain free text: the error line must start with
    /// `error[<code>]: <cause_prefix>`; the next line must be the exact fix line.
    pub fn assert_error_prefix(&self, exit: i32, code: &str, cause_prefix: &str, fix: &str) {
        self.assert_code(exit);
        let want = format!("error[{code}]: {cause_prefix}");
        let lines: Vec<&str> = self.stderr.lines().collect();
        let idx = lines
            .iter()
            .position(|l| l.starts_with(&want))
            .unwrap_or_else(|| panic!("no stderr line starting with {want:?}\n{}", self.context()));
        let fix_line = format!("  fix: {fix}");
        assert_eq!(
            lines.get(idx + 1).map(|l| l.to_string()),
            Some(fix_line),
            "the fix line must directly follow the error line\n{}",
            self.context()
        );
    }

    pub fn assert_stderr_contains(&self, needle: &str) {
        assert!(self.stderr.contains(needle), "stderr must contain {needle:?}\n{}", self.context());
    }

    pub fn assert_stderr_lacks(&self, needle: &str) {
        assert!(
            !self.stderr.contains(needle),
            "stderr must not contain {needle:?}\n{}",
            self.context()
        );
    }
}

/// Runs a prepared command to completion with a timeout; stdout and stderr are captured.
pub fn run_timeout(cmd: &mut Command, limit: Duration) -> Out {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child =
        cmd.spawn().unwrap_or_else(|e| panic!("cannot start {:?}: {e}", cmd.get_program()));
    let mut so = child.stdout.take().expect("stdout pipe");
    let mut se = child.stderr.take().expect("stderr pipe");
    let (tx_o, rx_o) = mpsc::channel::<Vec<u8>>();
    let (tx_e, rx_e) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = so.read_to_end(&mut buf);
        let _ = tx_o.send(buf);
    });
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = se.read_to_end(&mut buf);
        let _ = tx_e.send(buf);
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait().expect("try_wait") {
            Some(st) => break st,
            None => {
                if start.elapsed() > limit {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("{:?} did not finish within {:?}", cmd.get_program(), limit);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };
    let grab = |rx: mpsc::Receiver<Vec<u8>>| -> String {
        match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => String::new(),
        }
    };
    Out { code: status.code().unwrap_or(-1), stdout: grab(rx_o), stderr: grab(rx_e) }
}

pub fn run(cmd: &mut Command) -> Out {
    run_timeout(cmd, Duration::from_secs(300))
}

/// Builder for one `narrate` invocation. Every invocation gets its own `NARRATE_DATA_DIR` and
/// starts from an environment stripped of `NARRATE_*`, `FAKE_WORKER_*` and `FAKE_UV_*`.
pub struct Nar {
    cmd: Command,
}

impl Nar {
    pub fn new(data_dir: &Path) -> Nar {
        let mut cmd = Command::new(narrate_exe());
        for (k, _) in std::env::vars_os() {
            let name = k.to_string_lossy().to_ascii_uppercase();
            if name.starts_with("NARRATE_")
                || name.starts_with("FAKE_WORKER_")
                || name.starts_with("FAKE_UV_")
            {
                cmd.env_remove(&k);
            }
        }
        cmd.env("NARRATE_DATA_DIR", data_dir);
        Nar { cmd }
    }

    pub fn arg<S: AsRef<std::ffi::OsStr>>(mut self, a: S) -> Nar {
        self.cmd.arg(a);
        self
    }

    pub fn args<I, S>(mut self, a: I) -> Nar
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        self.cmd.args(a);
        self
    }

    pub fn env<K: AsRef<std::ffi::OsStr>, V: AsRef<std::ffi::OsStr>>(mut self, k: K, v: V) -> Nar {
        self.cmd.env(k, v);
        self
    }

    pub fn env_remove<K: AsRef<std::ffi::OsStr>>(mut self, k: K) -> Nar {
        self.cmd.env_remove(k);
        self
    }

    pub fn dir(mut self, d: &Path) -> Nar {
        self.cmd.current_dir(d);
        self
    }

    /// `NARRATE_WORKER_CMD` = the fake worker.
    pub fn worker(self) -> Nar {
        self.env("NARRATE_WORKER_CMD", worker_exe())
    }

    /// `NARRATE_UV` = the fake uv.
    pub fn uv(self) -> Nar {
        self.env("NARRATE_UV", uv_exe())
    }

    pub fn log(self, p: &Path) -> Nar {
        self.env("FAKE_WORKER_LOG", p)
    }

    pub fn run(mut self) -> Out {
        run(&mut self.cmd)
    }

    pub fn run_timeout(mut self, limit: Duration) -> Out {
        run_timeout(&mut self.cmd, limit)
    }

    pub fn command(&mut self) -> &mut Command {
        &mut self.cmd
    }

    /// Starts narrate in its own process group (so its pid is the group id) with stdout and
    /// stderr redirected to files in `scratch`.
    pub fn spawn_group(mut self, scratch: &Path) -> Spawned {
        use std::os::windows::process::CommandExt;
        let out_path = scratch.join("narrate.stdout.txt");
        let err_path = scratch.join("narrate.stderr.txt");
        let so = File::create(&out_path).expect("stdout file");
        let se = File::create(&err_path).expect("stderr file");
        self.cmd
            .stdin(Stdio::null())
            .stdout(Stdio::from(so))
            .stderr(Stdio::from(se))
            .creation_flags(CREATE_NEW_PROCESS_GROUP);
        let child = self.cmd.spawn().expect("start narrate");
        Spawned { child, out_path, err_path }
    }
}

pub struct Spawned {
    pub child: Child,
    pub out_path: PathBuf,
    pub err_path: PathBuf,
}

impl Spawned {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Waits for exit; `None` when still running after `limit`.
    pub fn wait(&mut self, limit: Duration) -> Option<i32> {
        let start = Instant::now();
        loop {
            if let Some(st) = self.child.try_wait().expect("try_wait") {
                return Some(st.code().unwrap_or(-1));
            }
            if start.elapsed() >= limit {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn stderr(&self) -> String {
        fs::read(&self.err_path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    }

    pub fn stdout(&self) -> String {
        fs::read(&self.out_path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    }
}

impl Drop for Spawned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn wait_until(limit: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    loop {
        if cond() {
            return true;
        }
        if start.elapsed() >= limit {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `narrate-test-signal <process-group-id>`: sends CTRL_BREAK_EVENT; true when it exits 0.
pub fn send_ctrl_break(group_id: u32) -> bool {
    Command::new(signal_exe())
        .arg(group_id.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|st| st.success())
        .unwrap_or(false)
}

/// `narrate-test-signal alive <pid>`: exit 0 = running, exit 1 = gone.
pub fn pid_alive(pid: u64) -> bool {
    Command::new(signal_exe())
        .arg("alive")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|st| st.code() == Some(0))
        .unwrap_or(false)
}

/// Adds a deny-write ACE for the current user on a directory and removes it on drop, so the
/// temporary directory can be deleted (icacls <dir> /remove:d "%USERNAME%").
pub struct DenyWrite {
    dir: PathBuf,
    user: String,
}

impl DenyWrite {
    pub fn apply(dir: &Path) -> DenyWrite {
        let user = std::env::var("USERNAME").expect("USERNAME must be set to build a deny ACE");
        let status = Command::new("icacls")
            .arg(dir)
            .arg("/deny")
            .arg(format!("{user}:(W)"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run icacls");
        assert!(status.success(), "icacls /deny failed for {}", dir.display());
        DenyWrite { dir: dir.to_path_buf(), user }
    }
}

impl Drop for DenyWrite {
    fn drop(&mut self) {
        let _ = Command::new("icacls")
            .arg(&self.dir)
            .arg("/remove:d")
            .arg(&self.user)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

// ---------------------------------------------------------------------------------------
// File-system helpers
// ---------------------------------------------------------------------------------------

pub fn scratch() -> TempDir {
    tempfile::tempdir().expect("temporary directory")
}

pub fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(&p, bytes).expect("write fixture");
    p
}

pub fn write_text(dir: &Path, name: &str, text: &str) -> PathBuf {
    write_bytes(dir, name, text.as_bytes())
}

pub fn list_dir(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

pub fn walk(dir: &Path) -> Vec<PathBuf> {
    fn rec(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                out.push(p.clone());
                if p.is_dir() {
                    rec(&p, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    rec(dir, &mut out);
    out
}

pub fn is_empty_dir(dir: &Path) -> bool {
    list_dir(dir).is_empty()
}

pub fn read_lossy(p: &Path) -> String {
    String::from_utf8_lossy(&fs::read(p).unwrap_or_default()).into_owned()
}

/// Sets the last-write time of a file to `hours` hours ago.
pub fn age_file(path: &Path, hours: u64) {
    let f = OpenOptions::new().write(true).open(path).expect("open for set_modified");
    f.set_modified(SystemTime::now() - Duration::from_secs(hours * 3600)).expect("set_modified");
}

/// Files in `dir` whose names end with `.wav`, plus any `*.narrate-*.tmp` leftovers.
pub fn wav_or_tmp_files(dir: &Path) -> Vec<String> {
    list_dir(dir)
        .into_iter()
        .filter(|n| {
            n.to_ascii_lowercase().ends_with(".wav") || (n.contains(".narrate-") && n.ends_with(".tmp"))
        })
        .collect()
}

pub fn tmp_leftovers(dir: &Path) -> Vec<String> {
    list_dir(dir).into_iter().filter(|n| n.contains(".narrate-") && n.ends_with(".tmp")).collect()
}

// ---------------------------------------------------------------------------------------
// Fabricated ready data directory (spec-narrate-test-strategy.md)
// ---------------------------------------------------------------------------------------

/// Every path of the readiness table (spec-narrate-setup.md Requirement 7), relative to D.
pub fn readiness_paths() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vec![
        ["venv", "pyvenv.cfg"].iter().collect(),
        ["venv", "Scripts", "python.exe"].iter().collect(),
        ["venv", "Lib", "site-packages", "chatterbox_tts-0.1.7.dist-info", "METADATA"]
            .iter()
            .collect(),
    ];
    for f in TURBO_FILES {
        v.push(["models", "chatterbox-turbo", f].iter().collect());
    }
    v.push(["models", "kokoro-82m", "kokoro-v1_0.pth"].iter().collect());
    v.push(["models", "kokoro-82m", "config.json"].iter().collect());
    v.push(["models", "kokoro-82m", "voices", "af_heart.pt"].iter().collect());
    v.push(["models", "kokoro-82m", "voices", "am_michael.pt"].iter().collect());
    for c in ["female.wav", "male.wav", "builtin.wav"] {
        v.push(["voices", c].iter().collect());
    }
    v
}

pub fn populate_ready_files(d: &Path) {
    for rel in readiness_paths() {
        let p = d.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"").unwrap();
    }
}

/// Reads the 64-hex id from `pin set: missing (binary <id>, recorded none)`.
pub fn parse_missing_pin_id(stdout: &str) -> String {
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("pin set: missing (binary ") {
            if let Some(id) = rest.strip_suffix(", recorded none)") {
                assert_eq!(id.len(), 64, "pin set id must be 64 hex characters: {id}");
                assert!(id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
                return id.to_string();
            }
        }
    }
    panic!("no `pin set: missing (binary <id>, recorded none)` line in:\n{stdout}");
}

/// The pin set id of the running binary (recipe step 2): `narrate status` against a second,
/// empty temporary directory.
pub fn binary_pin_set_id() -> String {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let empty = scratch();
        let out = Nar::new(empty.path()).arg("status").run();
        parse_missing_pin_id(&out.stdout)
    })
    .clone()
}

pub fn setup_json(id: &str) -> String {
    format!("{{\"format\":1,\"pin_set_id\":\"{id}\"}}")
}

/// A ready data directory built by the recipe of spec-narrate-test-strategy.md.
pub struct Ready {
    pub dir: TempDir,
    pub id: String,
}

impl Default for Ready {
    fn default() -> Ready {
        Ready::new()
    }
}

impl Ready {
    pub fn new() -> Ready {
        let dir = scratch();
        populate_ready_files(dir.path());
        let id = binary_pin_set_id();
        fs::write(dir.path().join("setup.json"), setup_json(&id)).unwrap();
        Ready { dir, id }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// `D` as narrate prints it: `std::path::absolute` of the data directory.
    pub fn d(&self) -> PathBuf {
        abs(self.dir.path())
    }

    /// A `narrate` command against this directory with the fake worker.
    pub fn nar(&self) -> Nar {
        Nar::new(self.path()).worker()
    }

    /// Removes one fabricated file; `rel` uses `/` separators.
    pub fn remove(&self, rel: &str) {
        let p = self.join(rel);
        fs::remove_file(&p).unwrap_or_else(|e| panic!("remove {}: {e}", p.display()));
    }

    pub fn join(&self, rel: &str) -> PathBuf {
        rel.split('/').fold(self.path().to_path_buf(), |acc, part| acc.join(part))
    }
}

// ---------------------------------------------------------------------------------------
// Worker log and environment dump readers
// ---------------------------------------------------------------------------------------

pub fn log_entries(path: &Path) -> Vec<Value> {
    let text = read_lossy(path);
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

/// All request entries (`event == "request"`), in order.
pub fn log_requests(path: &Path) -> Vec<Value> {
    log_entries(path).into_iter().filter(|e| e["event"] == "request").collect()
}

pub fn req_op(entry: &Value) -> String {
    entry["request"]["op"].as_str().unwrap_or("").to_string()
}

pub fn requests_of(path: &Path, op: &str) -> Vec<Value> {
    log_requests(path).into_iter().filter(|e| req_op(e) == op).collect()
}

pub fn op_sequence(path: &Path) -> Vec<String> {
    log_requests(path).iter().map(req_op).collect()
}

/// Text of every `synth` request, in order.
pub fn synth_texts(path: &Path) -> Vec<String> {
    requests_of(path, "synth")
        .iter()
        .map(|e| e["request"]["text"].as_str().unwrap_or("").to_string())
        .collect()
}

/// Distinct worker pids in the order they first appear in the log.
pub fn worker_pids(path: &Path) -> Vec<u64> {
    let mut seen: Vec<u64> = Vec::new();
    for e in log_entries(path) {
        if let Some(p) = e["pid"].as_u64() {
            if !seen.contains(&p) {
                seen.push(p);
            }
        }
    }
    seen
}

pub fn read_json(path: &Path) -> Value {
    serde_json::from_str(&read_lossy(path))
        .unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

/// Case-insensitive lookup in the `env` object of a worker or uv environment dump.
pub fn env_get(dump: &Value, name: &str) -> Option<String> {
    let env = dump.get("env")?.as_object()?;
    env.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .and_then(|(_, v)| v.as_str().map(|x| x.to_string()))
}

pub fn env_names(dump: &Value) -> Vec<String> {
    dump.get("env")
        .and_then(|e| e.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

/// Compares two paths the way the specs do: `std::path::absolute` plus ASCII case folding.
pub fn same_path(a: &str, b: &Path) -> bool {
    let a = abs(Path::new(a)).display().to_string().to_ascii_lowercase();
    let b = abs(b).display().to_string().to_ascii_lowercase();
    a == b
}

// ---------------------------------------------------------------------------------------
// verbose line parsing (spec-narrate-cli.md, Line catalog)
// ---------------------------------------------------------------------------------------

/// `verbose: chunk <i>/<N> samples=<n>`
pub fn parse_chunk_samples(line: &str) -> Option<(usize, usize, u64)> {
    let rest = line.strip_prefix("verbose: chunk ")?;
    let (idx, tail) = rest.split_once(' ')?;
    let (i, n) = idx.split_once('/')?;
    let samples = tail.strip_prefix("samples=")?;
    if samples.is_empty() || !samples.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((i.parse().ok()?, n.parse().ok()?, samples.parse().ok()?))
}

/// `verbose: silence after chunk <i> samples=<n>`
pub fn parse_silence(line: &str) -> Option<(usize, u64)> {
    let rest = line.strip_prefix("verbose: silence after chunk ")?;
    let (i, tail) = rest.split_once(' ')?;
    let samples = tail.strip_prefix("samples=")?;
    if samples.is_empty() || !samples.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((i.parse().ok()?, samples.parse().ok()?))
}

/// `verbose: chunk <i>/<N> text: <text>` returns `(i, N, text)`.
pub fn parse_chunk_text(line: &str) -> Option<(usize, usize, String)> {
    let rest = line.strip_prefix("verbose: chunk ")?;
    let (idx, tail) = rest.split_once(' ')?;
    let (i, n) = idx.split_once('/')?;
    let text = tail.strip_prefix("text: ")?;
    Some((i.parse().ok()?, n.parse().ok()?, text.to_string()))
}

pub struct VerboseRun {
    pub chunk_samples: Vec<(usize, usize, u64)>,
    pub silences: Vec<(usize, u64)>,
    pub texts: Vec<(usize, usize, String)>,
}

pub fn parse_verbose(out: &Out) -> VerboseRun {
    VerboseRun {
        chunk_samples: out.stderr.lines().filter_map(parse_chunk_samples).collect(),
        silences: out.stderr.lines().filter_map(parse_silence).collect(),
        texts: out.stderr.lines().filter_map(parse_chunk_text).collect(),
    }
}

/// The pid on the `verbose: worker pid <pid>` line.
pub fn verbose_worker_pid(out: &Out) -> Option<u64> {
    out.stderr
        .lines()
        .find_map(|l| l.strip_prefix("verbose: worker pid "))
        .and_then(|p| p.trim().parse().ok())
}

// ---------------------------------------------------------------------------------------
// WAV helpers
// ---------------------------------------------------------------------------------------

fn riff_chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(id);
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(body);
    if body.len() % 2 == 1 {
        v.push(0);
    }
    v
}

fn riff_fmt(tag: u16, channels: u16, rate: u32, bits: u16) -> Vec<u8> {
    let block_align = channels * bits.div_ceil(8);
    let mut b = Vec::new();
    b.extend_from_slice(&tag.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * block_align as u32).to_le_bytes());
    b.extend_from_slice(&block_align.to_le_bytes());
    b.extend_from_slice(&bits.to_le_bytes());
    b
}

fn riff_file(fmt: &[u8], data: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    body.extend_from_slice(&riff_chunk(b"fmt ", fmt));
    body.extend_from_slice(&riff_chunk(b"data", data));
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(&body);
    v
}

/// A WAV with `frames` frames of low-level noise in the given format (`tag` 1 = PCM,
/// 3 = IEEE float, 6 = A-law and so on).
pub fn wav_bytes(tag: u16, channels: u16, rate: u32, bits: u16, frames: u64) -> Vec<u8> {
    let block_align = channels as u64 * bits.div_ceil(8) as u64;
    let data: Vec<u8> = (0..frames * block_align).map(|i| (i % 7) as u8).collect();
    riff_file(&riff_fmt(tag, channels, rate, bits), &data)
}

/// 16-bit mono PCM clip of `frames` frames.
pub fn pcm16_clip(rate: u32, frames: u64) -> Vec<u8> {
    wav_bytes(1, 1, rate, 16, frames)
}

/// A clip that declares `declared` data bytes but holds only `present` of them.
pub fn truncated_clip(rate: u32, declared: u32, present: usize) -> Vec<u8> {
    let mut v = pcm16_clip(rate, (present / 2) as u64);
    v[40..44].copy_from_slice(&declared.to_le_bytes());
    v
}

/// `(channels, sample_rate, bits, is_int, total sample count over all channels)`
pub fn wav_facts(path: &Path) -> (u16, u32, u16, bool, u32) {
    let r = hound::WavReader::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let spec = r.spec();
    (
        spec.channels,
        spec.sample_rate,
        spec.bits_per_sample,
        spec.sample_format == hound::SampleFormat::Int,
        r.len(),
    )
}

pub fn wav_samples(path: &Path) -> u32 {
    wav_facts(path).4
}

/// Checks the format of an output WAV (Requirement 11): 1 channel, 24000 Hz, 16-bit integer,
/// file size = 44 + 2 x samples, 44-byte header followed directly by `data`.
pub fn assert_output_wav_format(path: &Path) -> u32 {
    let (ch, rate, bits, is_int, samples) = wav_facts(path);
    assert_eq!((ch, rate, bits, is_int), (1, 24_000, 16, true), "output WAV spec");
    let bytes = fs::read(path).unwrap();
    assert_eq!(bytes.len() as u64, 44 + 2 * samples as u64, "file size = 44 + 2 x samples");
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..16], b"WAVEfmt ");
    assert_eq!(&bytes[36..40], b"data", "no chunk between fmt and data");
    samples
}

// ---------------------------------------------------------------------------------------
// SHA-256 (the narrate dev-dependencies contain no hash crate)
// ---------------------------------------------------------------------------------------

pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (word, bytes) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *word = u32::from_be_bytes(*bytes);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for (k, wi) in K.iter().zip(w.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(*k).wrapping_add(*wi);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    h.iter().map(|x| format!("{x:08x}")).collect::<String>()
}

pub fn sha256_file(path: &Path) -> String {
    sha256_hex(&fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())))
}

// ---------------------------------------------------------------------------------------
// Repository files (read by pin-related tests)
// ---------------------------------------------------------------------------------------

pub fn real_pins_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("pins.json")
}

pub fn real_lock_path() -> PathBuf {
    repo_root().join("python").join("uv.lock")
}

pub fn real_pyproject_path() -> PathBuf {
    repo_root().join("python").join("pyproject.toml")
}

pub fn load_real_pins() -> Value {
    read_json(&real_pins_path())
}

/// Writes a modified copy of the committed `pins.json` and returns its path.
pub fn write_pins_variant(dir: &Path, name: &str, mutate: impl FnOnce(&mut Value)) -> PathBuf {
    let mut v = load_real_pins();
    mutate(&mut v);
    write_text(dir, name, &serde_json::to_string_pretty(&v).unwrap())
}

/// The pin set id the spec defines: sha256 of `narrate-pin-set-v1\n` + sha256hex(pins) + `\n` +
/// sha256hex(pyproject) + `\n` + sha256hex(lock) + `\n`.
pub fn compute_pin_set_id(pins: &[u8], pyproject: &[u8], lock: &[u8]) -> String {
    let text = format!(
        "narrate-pin-set-v1\n{}\n{}\n{}\n",
        sha256_hex(pins),
        sha256_hex(pyproject),
        sha256_hex(lock)
    );
    sha256_hex(text.as_bytes())
}

/// `(path, size, sha256)` of one pinned file.
pub type PinFile = (String, u64, String);

/// `(repo, dir, revision, files)` of one Hugging Face repository in a pins value.
pub type RepoPins = (String, String, String, Vec<PinFile>);

pub fn pin_files(pins: &Value) -> Vec<RepoPins> {
    pins["huggingface"]
        .as_array()
        .expect("huggingface array")
        .iter()
        .map(|r| {
            let files = r["files"]
                .as_array()
                .expect("files array")
                .iter()
                .map(|f| {
                    (
                        f["path"].as_str().unwrap().to_string(),
                        f["size"].as_u64().unwrap(),
                        f["sha256"].as_str().unwrap().to_string(),
                    )
                })
                .collect();
            (
                r["repo"].as_str().unwrap().to_string(),
                r["dir"].as_str().unwrap().to_string(),
                r["revision"].as_str().unwrap().to_string(),
                files,
            )
        })
        .collect()
}

/// Self-check of the hash helper against published vectors.
pub fn assert_sha256_helper_works() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    // 56-byte message: forces a second padding block.
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}
