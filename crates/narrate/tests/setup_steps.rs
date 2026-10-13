//! Setup steps 2 to 6 and setup interruption with the fake uv and the fake worker, without
//! network, uv, Python or GPU (FR-010, NFR-001, NFR-002, FR-011, FR-017).
//!
//! Sources: specs/spec-narrate-test-strategy.md (Requirement: Automated setup-step tests),
//! specs/spec-narrate-setup.md (Requirements 2 to 6 and 9, Child process environment, Pin set
//! identifier), specs/spec-narrate-worker.md (Requirement 9). Every `narrate` invocation of a
//! case sets the same `NARRATE_TEST_PINS_FILE`, `NARRATE_UV`, `NARRATE_WORKER_CMD` and
//! `NARRATE_DATA_DIR`, because the pin set id is computed from the effective `pins.json`.
//! The test pins file is the committed `crates/narrate/pins.json` with the 13 file sizes and
//! sha256 values replaced by those of small fixture files the fake worker serves, and with
//! `python.sha256` replaced by the sha256 of the archive the fake uv copies into
//! `UV_PYTHON_CACHE_DIR`. The embedded `uv.lock` stays in effect. No implementation was read.

mod common;

use common::*;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const FIX_NETWORK: &str = "check the network connection and run narrate setup again; completed steps are kept";
const FIX_HASH: &str = "run narrate setup again; if this repeats, do not use this network";
const CLIPS: [&str; 3] = ["female", "male", "builtin"];

// ---------------------------------------------------------------------------------------
// Sandbox: fixtures, test pins, fakes
// ---------------------------------------------------------------------------------------

#[derive(Default, Clone)]
struct Opts {
    wrong_python_sha: bool,
    wrong_sha_for: Option<&'static str>,
    skip_fixture: Option<&'static str>,
}

struct Sb {
    root: TempDir,
    data: TempDir,
    pins: PathBuf,
    dl_dir: PathBuf,
    archive: PathBuf,
    python_url: String,
    wrong_python_sha: String,
}

fn fixture_bytes(repo: &str, path: &str) -> Vec<u8> {
    format!("fixture::{repo}::{path}\n").into_bytes()
}

fn write_fixture(dl_dir: &Path, repo: &str, path: &str) {
    write_bytes(dl_dir, &format!("{}/{}", repo.replace('/', "__"), path), &fixture_bytes(repo, path));
}

impl Sb {
    fn new() -> Sb {
        Sb::with(Opts::default())
    }

    fn with(opts: Opts) -> Sb {
        let root = scratch();
        let data = scratch();
        let dl_dir = root.path().join("fixtures");
        fs::create_dir_all(&dl_dir).unwrap();
        let archive = write_bytes(root.path(), "python-archive.tar.gz", b"pretend python-build-standalone archive");
        let archive_sha = sha256_hex(&fs::read(&archive).unwrap());
        let wrong_python_sha = sha256_hex(b"not the archive");

        let mut pins = load_real_pins();
        for repo in pins["huggingface"].as_array_mut().expect("huggingface array").iter_mut() {
            let name = repo["repo"].as_str().unwrap().to_string();
            for f in repo["files"].as_array_mut().expect("files array").iter_mut() {
                let path = f["path"].as_str().unwrap().to_string();
                let bytes = fixture_bytes(&name, &path);
                f["size"] = json!(bytes.len() as u64);
                let sha = if opts.wrong_sha_for == Some(path.as_str()) {
                    sha256_hex(b"not the fixture")
                } else {
                    sha256_hex(&bytes)
                };
                f["sha256"] = json!(sha);
                if opts.skip_fixture != Some(path.as_str()) {
                    write_fixture(&dl_dir, &name, &path);
                }
            }
        }
        pins["python"]["sha256"] = json!(if opts.wrong_python_sha { wrong_python_sha.clone() } else { archive_sha });
        let python_url = pins["python"]["url"].as_str().unwrap().to_string();
        let pins_path = write_text(root.path(), "pins-test.json", &serde_json::to_string_pretty(&pins).unwrap());
        Sb { root, data, pins: pins_path, dl_dir, archive, python_url, wrong_python_sha }
    }

    fn d(&self) -> PathBuf {
        abs(self.data.path())
    }

    fn join(&self, rel: &str) -> PathBuf {
        rel.split('/').fold(self.data.path().to_path_buf(), |acc, part| acc.join(part))
    }

    fn uv_log(&self, tag: &str) -> PathBuf {
        self.root.path().join(format!("uv-{tag}.log"))
    }

    fn uv_dump(&self, tag: &str) -> PathBuf {
        self.root.path().join(format!("uvenv-{tag}.jsonl"))
    }

    fn w_log(&self, tag: &str) -> PathBuf {
        self.root.path().join(format!("worker-{tag}.log"))
    }

    /// Every invocation gets the same four selectors plus per-run log paths.
    fn base(&self, tag: &str) -> Nar {
        Nar::new(self.data.path())
            .uv()
            .worker()
            .env("NARRATE_TEST_PINS_FILE", &self.pins)
            .env("FAKE_UV_LOG", self.uv_log(tag))
            .env("FAKE_UV_ENV_DUMP", self.uv_dump(tag))
            .env("FAKE_UV_PY_ARCHIVE", &self.archive)
            .env("FAKE_WORKER_LOG", self.w_log(tag))
            .env("FAKE_WORKER_ENV_DUMP", self.root.path().join(format!("wenv-{tag}-{{pid}}.json")))
            .env("FAKE_WORKER_DOWNLOAD_DIR", &self.dl_dir)
            // 223 characters x 1100 = 245300 samples: the builtin clip reaches 240000 at once.
            .env("FAKE_WORKER_SAMPLES_PER_CHAR", "1100")
    }

    fn setup(&self, tag: &str) -> Nar {
        self.base(tag).arg("setup")
    }

    fn status(&self) -> Out {
        self.base("status").arg("status").run()
    }

    fn pin_id(&self) -> String {
        compute_pin_set_id(
            &fs::read(&self.pins).unwrap(),
            &fs::read(real_pyproject_path()).unwrap(),
            &fs::read(real_lock_path()).unwrap(),
        )
    }

    fn clip_path(&self, name: &str) -> PathBuf {
        self.data.path().join("voices").join(format!("{name}.wav"))
    }

    fn clip_hashes(&self) -> Vec<String> {
        CLIPS.iter().map(|c| sha256_file(&self.clip_path(c))).collect()
    }

    fn setup_json_bytes(&self) -> Option<Vec<u8>> {
        fs::read(self.data.path().join("setup.json")).ok()
    }

    fn assert_status_ready(&self) {
        let out = self.status();
        out.assert_code(0);
        assert_eq!(out.stdout_lines().last().copied(), Some("status: ready"), "{}", out.context());
        let id = self.pin_id();
        assert!(
            out.stdout.lines().any(|l| l == format!("pin set: match (binary {id}, recorded {id})")),
            "{}",
            out.context()
        );
    }

    /// A complete fake setup; returns the clip hashes.
    fn complete(&self) -> Vec<String> {
        let out = self.setup("complete").run();
        out.assert_code(0);
        assert_eq!(out.stderr_lines().last().copied(), Some("setup: ready"), "{}", out.context());
        self.clip_hashes()
    }
}

fn step(k: usize, name: &str, result: &str) -> String {
    format!("setup: [{k}/6] {name}: {result}")
}

fn setup_lines(out: &Out) -> Vec<String> {
    out.stderr.lines().filter(|l| l.starts_with("setup: ")).map(|l| l.to_string()).collect()
}

fn lines_of(log: &Path) -> Vec<String> {
    read_lossy(log).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

fn uv_calls(log: &Path, what: &str) -> Vec<String> {
    lines_of(log).into_iter().filter(|l| l.starts_with(what)).collect()
}

/// Worker requests grouped by worker pid, in session order.
fn sessions(log: &Path) -> Vec<(u64, Vec<Value>)> {
    let mut out: Vec<(u64, Vec<Value>)> = Vec::new();
    for e in log_requests(log) {
        let pid = e["pid"].as_u64().unwrap_or(0);
        match out.iter_mut().find(|(p, _)| *p == pid) {
            Some((_, v)) => v.push(e),
            None => out.push((pid, vec![e])),
        }
    }
    out
}

fn ops_of(entries: &[Value]) -> Vec<String> {
    entries.iter().map(req_op).collect()
}

fn uv_env_dumps(path: &Path) -> Vec<Value> {
    read_lossy(path).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect()
}

fn names_under(dir: &Path, file_name: &str) -> Vec<PathBuf> {
    walk(dir).into_iter().filter(|p| p.file_name().map(|n| n == file_name).unwrap_or(false)).collect()
}

// ---------------------------------------------------------------------------------------
// First run and re-run
// ---------------------------------------------------------------------------------------

// [Scenario: First run and re-run with the fakes (automated) | spec-narrate-setup Req 6 / spec-narrate-test-strategy | FR-010 | FR-017 | NFR-001 | NFR-008]
#[test]
fn first_run_downloads_everything_and_ends_ready_then_status_exits_0() {
    let sb = Sb::new();
    // Entries narrate does not own must survive (RULE-004, Phase 2 data lives in onnx\).
    let onnx_keep = write_bytes(sb.data.path(), "onnx/keep.bin", b"phase 2");
    let unknown = write_bytes(sb.data.path(), "unknown.txt", b"not ours");

    let out = sb.setup("run1").run();
    out.assert_code(0);
    assert_eq!(out.stdout, "", "setup leaves stdout empty");
    let id = sb.pin_id();
    let expected = vec![
        format!("setup: data directory {}", sb.d().display()),
        step(1, "preflight", "ok (uv 0.10.2)"),
        step(2, "python", "ok (downloaded)"),
        step(3, "environment", "ok (downloaded)"),
        step(4, "models", "ok (downloaded)"),
        step(5, "voices", "ok (generated female, male, builtin)"),
        step(6, "record", &format!("ok (pin set {})", &id[..12])),
        "setup: ready".to_string(),
    ];
    assert_eq!(setup_lines(&out), expected, "{}", out.context());
    assert_eq!(out.stderr_lines().first().copied(), Some(expected[0].as_str()), "the data directory line comes first");
    assert_eq!(out.stderr_lines().last().copied(), Some("setup: ready"));

    sb.assert_status_ready();

    // All 13 pinned files are in place with the fixture bytes; s3gen.safetensors is never fetched.
    let pins = load_real_pins();
    let mut count = 0;
    for (repo, dir, _rev, files) in pin_files(&pins) {
        for (path, _size, _sha) in files {
            let final_path = path.split('/').fold(sb.join(&format!("models/{dir}")), |acc, p| acc.join(p));
            assert_eq!(fs::read(&final_path).unwrap(), fixture_bytes(&repo, &path), "{repo}/{path}");
            count += 1;
        }
    }
    assert_eq!(count, 13);
    assert!(!sb.join("models/chatterbox-turbo/s3gen.safetensors").exists());
    assert!(!sb.join("staging/models").exists(), "staging\\models is deleted after all files verify");

    // Clips: 24 kHz mono 16-bit, at least 10 s.
    for c in CLIPS {
        let (ch, rate, bits, is_int, samples) = wav_facts(&sb.clip_path(c));
        assert_eq!((ch, rate, bits, is_int), (1, 24_000, 16, true), "{c}");
        assert!(samples >= 240_000, "{c} has {samples} samples");
    }

    // Record (Requirement 6).
    let record = read_json(&sb.join("setup.json"));
    assert_eq!(record["format"], 1);
    assert_eq!(record["pin_set_id"], id.as_str());
    assert_eq!(record["narrate_version"], "0.1.0");
    assert!(record["completed_unix"].as_u64().unwrap() > 1_700_000_000);
    for c in CLIPS {
        assert_eq!(record["clips"][c]["sha256"], sha256_file(&sb.clip_path(c)).as_str(), "{c}");
        assert_eq!(record["clips"][c]["samples"].as_u64(), Some(wav_samples(&sb.clip_path(c)) as u64), "{c}");
    }

    // Project copies, log, lock, temporary directory.
    assert_eq!(fs::read(sb.join("project/pyproject.toml")).unwrap(), fs::read(real_pyproject_path()).unwrap());
    assert_eq!(fs::read(sb.join("project/uv.lock")).unwrap(), fs::read(real_lock_path()).unwrap());
    assert!(sb.join("project/pins.json").is_file());
    assert!(sb.join("setup.lock").exists());
    let setup_log = read_lossy(&sb.join("logs/setup-last.log"));
    assert!(setup_log.contains("started pid"), "worker output goes to logs\\setup-last.log");
    assert!(!sb.join("tmp").exists(), "D\\tmp and the run directory are removed when the command ends");
    assert_eq!(fs::read(&onnx_keep).unwrap(), b"phase 2");
    assert_eq!(fs::read(&unknown).unwrap(), b"not ours");
}

// [Scenario: Python step with the fake uv (automated) | spec-narrate-setup Req 2 | FR-010 | NFR-001]
#[test]
fn python_install_runs_offline_first_then_online_and_a_rerun_stays_offline() {
    let sb = Sb::new();
    sb.setup("run1").run().assert_code(0);
    let log1 = sb.uv_log("run1");
    let installs = uv_calls(&log1, "python install");
    assert_eq!(installs.len(), 2, "first run: once with --offline, then once without\n{:?}", lines_of(&log1));
    assert!(installs[0].contains("--offline"), "{}", installs[0]);
    assert!(!installs[1].contains("--offline"), "{}", installs[1]);
    for line in &installs {
        for flag in ["3.11.14", "--no-bin", "--no-registry"] {
            assert!(line.contains(flag), "{flag} in {line}");
        }
    }
    let syncs = uv_calls(&log1, "sync");
    assert_eq!(syncs.len(), 2, "{:?}", lines_of(&log1));
    assert!(syncs[0].contains("--offline") && !syncs[1].contains("--offline"));
    for line in &syncs {
        for flag in ["--project", "--frozen", "--managed-python", "--python 3.11.14", "--no-progress"] {
            assert!(line.contains(flag), "{flag} in {line}");
        }
        assert!(line.contains(&sb.join("project").display().to_string()), "project directory in {line}");
    }
    // Order: --version, python install (offline, online), sync (offline, online).
    let kinds: Vec<String> = lines_of(&log1)
        .iter()
        .map(|l| if l.starts_with("--version") { "version" } else if l.starts_with("python install") { "python" } else if l.starts_with("sync") { "sync" } else { "other" }.to_string())
        .collect();
    assert_eq!(kinds, vec!["version", "python", "python", "sync", "sync"]);

    let out2 = sb.setup("run2").run();
    out2.assert_code(0);
    assert!(out2.has_stderr_line(&step(2, "python", "ok (no network used)")), "{}", out2.context());
    let log2 = sb.uv_log("run2");
    let installs2 = uv_calls(&log2, "python install");
    assert_eq!(installs2.len(), 1, "{:?}", lines_of(&log2));
    assert!(installs2[0].contains("--offline"));
    let syncs2 = uv_calls(&log2, "sync");
    assert_eq!(syncs2.len(), 1);
    assert!(syncs2[0].contains("--offline"));
}

// [Scenario: First run and re-run with the fakes (automated) | spec-narrate-setup Req 6 | FR-010]
#[test]
fn rerun_on_a_complete_setup_uses_no_network_and_leaves_the_clips_byte_identical() {
    let sb = Sb::new();
    let before = sb.complete();
    let out = sb.setup("run2").run();
    out.assert_code(0);
    let id = sb.pin_id();
    let expected = vec![
        format!("setup: data directory {}", sb.d().display()),
        step(1, "preflight", "ok (uv 0.10.2)"),
        step(2, "python", "ok (no network used)"),
        step(3, "environment", "ok (no network used)"),
        step(4, "models", "ok (no network used)"),
        step(5, "voices", "ok (all present)"),
        step(6, "record", &format!("ok (pin set {})", &id[..12])),
        "setup: ready".to_string(),
    ];
    assert_eq!(setup_lines(&out), expected, "{}", out.context());
    let ops: Vec<String> = log_requests(&sb.w_log("run2")).iter().map(req_op).collect();
    assert!(!ops.iter().any(|o| o == "download" || o == "kokoro_clip"), "no download or kokoro_clip request: {ops:?}");
    assert_eq!(sb.clip_hashes(), before, "clips are byte-identical");
    sb.assert_status_ready();
}

// [Requirement 6: Record matches binary | spec-narrate-setup | FR-017]
#[test]
fn status_without_the_test_pins_file_reports_a_mismatch_after_a_fake_setup() {
    let sb = Sb::new();
    sb.complete();
    let out = Nar::new(sb.data.path()).worker().arg("status").run();
    out.assert_code(4);
    let want = format!("pin set: mismatch (binary {}, recorded {})", binary_pin_set_id(), sb.pin_id());
    assert!(out.stdout.lines().any(|l| l == want), "{}", out.context());
    assert_eq!(out.stdout_lines().last().copied(), Some("status: not ready (run narrate setup)"));
}

// [Scenario: Verbose setup output | spec-narrate-setup Algorithmic steps | FR-010 | FR-024]
#[test]
fn verbose_setup_prints_child_pids_and_prefixed_child_output() {
    let sb = Sb::new();
    let out = sb.setup("run1").arg("--verbose").run();
    out.assert_code(0);
    let uv_pids: Vec<&str> = out.stderr.lines().filter_map(|l| l.strip_prefix("verbose: uv pid ")).collect();
    assert!(uv_pids.len() >= 5, "preflight --version, two python installs, two syncs: {uv_pids:?}\n{}", out.context());
    assert!(uv_pids.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())));
    let worker_pids: Vec<u64> = out.stderr.lines().filter_map(|l| l.strip_prefix("verbose: worker pid ")).map(|p| p.parse().unwrap()).collect();
    assert_eq!(worker_pids, sessions(&sb.w_log("run1")).iter().map(|(p, _)| *p).collect::<Vec<_>>(), "one line per worker session");
    assert!(out.stderr.lines().any(|l| l.starts_with("worker: ") && l.contains("started pid")), "{}", out.context());

    let quiet = Sb::new();
    let q = quiet.setup("run1").run();
    q.assert_code(0);
    assert!(!q.stderr.lines().any(|l| l.starts_with("worker: ") || l.starts_with("uv: ") || l.starts_with("verbose:")), "{}", q.context());
}

// ---------------------------------------------------------------------------------------
// Hash mismatches (NFR-001)
// ---------------------------------------------------------------------------------------

// [Scenario: Archive hash mismatch (automated) | spec-narrate-setup Req 2 | NFR-001 | FR-011]
#[test]
fn python_archive_hash_mismatch_fails_names_the_url_and_removes_both_directories() {
    let sb = Sb::with(Opts { wrong_python_sha: true, ..Opts::default() });
    let out = sb.setup("run1").run();
    out.assert_code(5);
    let line = out
        .stderr
        .lines()
        .find(|l| l.starts_with("error[hash-mismatch]: "))
        .unwrap_or_else(|| panic!("{}", out.context()))
        .to_string();
    assert!(line.contains(&sb.python_url), "the pinned URL is named: {line}");
    assert!(line.contains("does not match its pinned sha256"), "{line}");
    assert!(line.contains(&sb.wrong_python_sha), "expected hash shown: {line}");
    assert!(line.contains(&sha256_file(&sb.archive)), "actual hash shown: {line}");
    assert!(line.ends_with("the file was discarded"), "{line}");
    assert!(out.has_stderr_line(&format!("  fix: {FIX_HASH}")), "{}", out.context());
    assert!(!sb.join("python").exists(), "D\\python must not exist");
    assert!(!sb.join("python-cache").exists(), "D\\python-cache must not exist");
    assert!(sb.setup_json_bytes().is_none());
    assert!(uv_calls(&sb.uv_log("run1"), "sync").is_empty(), "the environment step never runs");
}

// [Scenario: Model hash mismatch with the fake worker (automated) | spec-narrate-setup Req 4 | NFR-001 | FR-011]
#[test]
fn model_hash_mismatch_names_repo_and_file_and_discards_the_download() {
    let sb = Sb::with(Opts { wrong_sha_for: Some("added_tokens.json"), ..Opts::default() });
    let out = sb.setup("run1").run();
    out.assert_code(5);
    let line = out
        .stderr
        .lines()
        .find(|l| l.starts_with("error[hash-mismatch]: "))
        .unwrap_or_else(|| panic!("{}", out.context()))
        .to_string();
    assert!(line.contains("ResembleAI/chatterbox-turbo/added_tokens.json"), "{line}");
    assert!(line.contains("does not match its pinned sha256"), "{line}");
    assert!(out.has_stderr_line(&format!("  fix: {FIX_HASH}")), "{}", out.context());
    assert!(names_under(&sb.join("staging"), "added_tokens.json").is_empty(), "no staged copy may remain");
    assert!(!sb.join("models/chatterbox-turbo/added_tokens.json").exists(), "a mismatching file is never renamed into place");
    assert!(sb.join("models/chatterbox-turbo/t3_turbo_v1.safetensors").is_file(), "files verified before the mismatch are kept");
    assert!(sb.setup_json_bytes().is_none(), "D\\setup.json must not exist");
}

// [Scenario: Model hash mismatch, existing final file | spec-narrate-setup Req 4 | NFR-001]
#[test]
fn model_hash_mismatch_leaves_an_existing_final_file_untouched() {
    let sb = Sb::with(Opts { wrong_sha_for: Some("added_tokens.json"), ..Opts::default() });
    let final_path = write_bytes(sb.data.path(), "models/chatterbox-turbo/added_tokens.json", b"old content that does not match any pin");
    let out = sb.setup("run1").run();
    out.assert_code(5);
    out.assert_stderr_contains("error[hash-mismatch]");
    assert_eq!(fs::read(&final_path).unwrap(), b"old content that does not match any pin");
    assert!(names_under(&sb.join("staging"), "added_tokens.json").is_empty());
}

// [Requirement 4: keep verified final files, replace the rest | spec-narrate-setup | FR-010 | NFR-001]
#[test]
fn only_missing_or_unverified_model_files_are_downloaded() {
    let sb = Sb::new();
    let good = write_bytes(sb.data.path(), "models/chatterbox-turbo/conds.pt", &fixture_bytes(TURBO_REPO, "conds.pt"));
    let bad = write_bytes(sb.data.path(), "models/chatterbox-turbo/ve.safetensors", b"corrupt");
    let out = sb.setup("run1").run();
    out.assert_code(0);
    let downloaded: Vec<String> = requests_of(&sb.w_log("run1"), "download")
        .iter()
        .map(|e| e["request"]["file"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(downloaded.len(), 12, "all but the verified conds.pt: {downloaded:?}");
    assert!(!downloaded.iter().any(|f| f == "conds.pt"));
    assert!(downloaded.iter().any(|f| f == "ve.safetensors"));
    assert_eq!(fs::read(&good).unwrap(), fixture_bytes(TURBO_REPO, "conds.pt"));
    assert_eq!(fs::read(&bad).unwrap(), fixture_bytes(TURBO_REPO, "ve.safetensors"), "the corrupt file is replaced");
}

// [Requirement 4: download requests | spec-narrate-setup / spec-narrate-worker Req 8 | FR-010 | FR-011]
#[test]
fn download_requests_carry_the_pinned_repo_revision_file_and_staging_directory() {
    let sb = Sb::new();
    sb.setup("run1").run().assert_code(0);
    let log = sb.w_log("run1");
    let all = sessions(&log);
    assert_eq!(all.len(), 2, "one download session and one voices session");
    let (_, download_session) = &all[0];
    let ops = ops_of(download_session);
    assert_eq!(ops.first().map(String::as_str), Some("hello"));
    assert_eq!(ops.last().map(String::as_str), Some("shutdown"));
    assert_eq!(ops.iter().filter(|o| *o == "download").count(), 13);

    let pins = load_real_pins();
    let mut expected: Vec<(String, String, String, PathBuf)> = Vec::new();
    for (repo, dir, rev, files) in pin_files(&pins) {
        for (path, _, _) in files {
            expected.push((repo.clone(), rev.clone(), path, sb.join(&format!("staging/models/{dir}"))));
        }
    }
    let requests: Vec<&Value> = download_session.iter().filter(|e| req_op(e) == "download").collect();
    for (req, (repo, rev, path, local_dir)) in requests.iter().zip(expected.iter()) {
        let r = &req["request"];
        assert_eq!(r["repo"].as_str(), Some(repo.as_str()));
        assert_eq!(r["revision"].as_str(), Some(rev.as_str()), "pinned revision for {repo}");
        assert_eq!(r["file"].as_str(), Some(path.as_str()), "repo-relative path with /");
        assert!(same_path(r["local_dir"].as_str().unwrap(), local_dir), "local_dir {} vs {}", r["local_dir"], local_dir.display());
    }
}

// [Scenario: Worker returns a path outside staging (automated) | spec-narrate-setup Req 4 | NFR-001]
#[test]
fn a_download_reply_outside_staging_fails_the_step_and_deletes_nothing() {
    let sb = Sb::new();
    let outside = write_bytes(sb.root.path(), "outside.bin", b"must survive");
    let out = sb.setup("run1").env("FAKE_WORKER_DOWNLOAD_PATH_OVERRIDE", &outside).run();
    out.assert_code(5);
    let line = out
        .stderr
        .lines()
        .find(|l| l.starts_with("error[setup-step-failed]: "))
        .unwrap_or_else(|| panic!("{}", out.context()))
        .to_string();
    assert!(line.contains("setup failed at step models: the worker returned a path outside the staging directory: "), "{line}");
    assert!(line.contains("outside.bin"), "{line}");
    assert!(out.has_stderr_line(&format!("  fix: {FIX_NETWORK}")), "{}", out.context());
    assert_eq!(fs::read(&outside).unwrap(), b"must survive", "the file at that path still exists");
    assert!(sb.setup_json_bytes().is_none());
}

// [Requirement 4: download_failed keeps staging for resume | spec-narrate-setup | FR-010 | UR-003]
#[test]
fn a_failed_download_keeps_progress_and_a_rerun_fetches_only_the_rest() {
    let sb = Sb::with(Opts { skip_fixture: Some("vocab.json"), ..Opts::default() });
    let out = sb.setup("run1").run();
    out.assert_code(5);
    let line = out
        .stderr
        .lines()
        .find(|l| l.starts_with("error[setup-step-failed]: setup failed at step models: "))
        .unwrap_or_else(|| panic!("{}", out.context()))
        .to_string();
    assert!(line.contains("vocab.json"), "{line}");
    assert!(out.has_stderr_line(&format!("  fix: {FIX_NETWORK}")), "{}", out.context());
    assert!(sb.join("staging/models").exists(), "staging\\models is kept so a re-run resumes");
    assert!(sb.setup_json_bytes().is_none());
    let status = sb.status();
    status.assert_code(4);
    assert_eq!(status.stdout_lines().last().copied(), Some("status: not ready (run narrate setup)"));
    // The four files before vocab.json were verified and renamed into place.
    for done in ["t3_turbo_v1.safetensors", "s3gen_meanflow.safetensors", "ve.safetensors", "conds.pt"] {
        assert!(sb.join(&format!("models/chatterbox-turbo/{done}")).is_file(), "{done}");
    }

    // The network is back: the missing fixture appears and setup is run again.
    write_fixture(&sb.dl_dir, TURBO_REPO, "vocab.json");
    let out2 = sb.setup("run2").run();
    out2.assert_code(0);
    assert!(out2.has_stderr_line(&step(2, "python", "ok (no network used)")), "{}", out2.context());
    assert!(out2.has_stderr_line(&step(3, "environment", "ok (no network used)")), "{}", out2.context());
    assert!(out2.has_stderr_line(&step(4, "models", "ok (downloaded)")), "{}", out2.context());
    assert_eq!(out2.stderr_lines().last().copied(), Some("setup: ready"));
    let downloaded: Vec<String> = requests_of(&sb.w_log("run2"), "download")
        .iter()
        .map(|e| e["request"]["file"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(downloaded.len(), 9, "only the 9 files that were not yet in place: {downloaded:?}");
    assert!(downloaded.iter().any(|f| f == "vocab.json"));
    for done in ["t3_turbo_v1.safetensors", "s3gen_meanflow.safetensors", "ve.safetensors", "conds.pt"] {
        assert!(!downloaded.iter().any(|f| f == done), "{done} must not be downloaded again");
    }
    assert!(!sb.join("staging/models").exists());
    sb.assert_status_ready();
}

// ---------------------------------------------------------------------------------------
// Voices
// ---------------------------------------------------------------------------------------

// [Scenario: First run and re-run with the fakes: voice session | spec-narrate-setup Req 5 | FR-010 | FR-008]
#[test]
fn clips_are_rendered_in_one_offline_session_in_the_documented_order() {
    let sb = Sb::new();
    sb.setup("run1").run().assert_code(0);
    let all = sessions(&sb.w_log("run1"));
    assert_eq!(all.len(), 2);
    let (_, voices) = &all[1];
    assert_eq!(ops_of(voices), vec!["hello", "kokoro_clip", "kokoro_clip", "load", "voice", "synth", "shutdown"]);
    let kokoro: Vec<&Value> = voices.iter().filter(|e| req_op(e) == "kokoro_clip").collect();
    assert_eq!(kokoro[0]["request"]["voice"], "af_heart");
    assert_eq!(kokoro[1]["request"]["voice"], "am_michael");
    for k in &kokoro {
        assert_eq!(k["request"]["text"].as_str(), Some(REFERENCE_TEXT));
        assert!(same_path(k["request"]["model_dir"].as_str().unwrap(), &sb.join("models/kokoro-82m")));
    }
    let load = voices.iter().find(|e| req_op(e) == "load").unwrap();
    assert_eq!(load["request"]["device"], "auto");
    assert!(same_path(load["request"]["model_dir"].as_str().unwrap(), &sb.join("models/chatterbox-turbo")));
    let voice = voices.iter().find(|e| req_op(e) == "voice").unwrap();
    assert_eq!(voice["request"]["voice_id"], "builtin");
    assert!(voice["request"]["clip"].is_null());
    let synth = voices.iter().find(|e| req_op(e) == "synth").unwrap();
    let r = &synth["request"];
    assert_eq!((r["chunk"].as_u64(), r["total"].as_u64(), r["seed"].as_u64()), (Some(1), Some(1), Some(1)));
    assert_eq!(r["voice_id"], "builtin");
    assert_eq!(r["text"].as_str(), Some(REFERENCE_TEXT));
    // Worker sessions: the download session is online, the voices session is offline.
    let dumps_dir = sb.root.path();
    let mut offline_flags = Vec::new();
    for (pid, _) in &all {
        let dump = read_json(&dumps_dir.join(format!("wenv-run1-{pid}.json")));
        offline_flags.push(env_get(&dump, "HF_HUB_OFFLINE"));
    }
    assert_eq!(offline_flags, vec![None, Some("1".to_string())], "download session online, voices session offline");
}

// [Scenario: One clip deleted (automated with the fakes) | spec-narrate-setup Req 5 | FR-010 | RULE-005]
#[test]
fn deleted_clips_are_regenerated_and_the_others_stay_byte_identical() {
    let sb = Sb::new();
    let original = sb.complete();
    let cases: [(&[&str], &str); 4] = [
        (&["male"], "ok (generated male)"),
        (&["builtin"], "ok (generated builtin)"),
        (&["female", "builtin"], "ok (generated female, builtin)"),
        (&["female"], "ok (generated female)"),
    ];
    for (i, (deleted, expected)) in cases.iter().enumerate() {
        for name in *deleted {
            fs::remove_file(sb.clip_path(name)).unwrap();
        }
        let tag = format!("again{i}");
        let out = sb.setup(&tag).run();
        out.assert_code(0);
        assert!(out.has_stderr_line(&step(5, "voices", expected)), "{deleted:?}\n{}", out.context());
        assert_eq!(sb.clip_hashes(), original, "regenerated clips come out identical and the kept ones are untouched ({deleted:?})");
        // Only the missing clips are rendered.
        let log = sb.w_log(&tag);
        let kokoro_voices: Vec<String> = requests_of(&log, "kokoro_clip").iter().map(|e| e["request"]["voice"].as_str().unwrap().to_string()).collect();
        let mut want = Vec::new();
        if deleted.contains(&"female") {
            want.push("af_heart".to_string());
        }
        if deleted.contains(&"male") {
            want.push("am_michael".to_string());
        }
        assert_eq!(kokoro_voices, want, "{deleted:?}");
        assert_eq!(requests_of(&log, "synth").len(), usize::from(deleted.contains(&"builtin")), "{deleted:?}");
        assert!(requests_of(&log, "download").is_empty());
        sb.assert_status_ready();
    }
}

// [Business rule RULE-005 | spec-narrate-setup | FR-010 | FR-017]
#[test]
fn clips_are_rerendered_when_the_record_is_missing_or_names_another_pin_set() {
    let sb = Sb::new();
    sb.complete();
    // A record for a different pin set.
    fs::write(sb.join("setup.json"), setup_json(&"0".repeat(64))).unwrap();
    let out = sb.setup("other").run();
    out.assert_code(0);
    assert!(out.has_stderr_line(&step(5, "voices", "ok (generated female, male, builtin)")), "{}", out.context());
    sb.assert_status_ready();
    // No record at all.
    fs::remove_file(sb.join("setup.json")).unwrap();
    let out = sb.setup("none").run();
    out.assert_code(0);
    assert!(out.has_stderr_line(&step(5, "voices", "ok (generated female, male, builtin)")), "{}", out.context());
    sb.assert_status_ready();
}

// [Scenario: Record written last (automated) | spec-narrate-setup Req 6 | FR-010 | FR-017]
#[test]
fn a_failing_voices_step_leaves_setup_json_byte_identical() {
    let sb = Sb::new();
    sb.complete();
    fs::remove_file(sb.clip_path("female")).unwrap();
    let before = sb.setup_json_bytes().expect("setup.json from the complete setup");
    let out = sb.setup("fail").env("FAKE_WORKER_FAIL_OP", "kokoro_clip:kokoro_failed").run();
    out.assert_code(5);
    assert!(
        out.stderr.lines().any(|l| l.starts_with("error[setup-step-failed]: setup failed at step voices: ")),
        "{}",
        out.context()
    );
    assert!(out.has_stderr_line(&format!("  fix: {FIX_NETWORK}")), "{}", out.context());
    assert_eq!(sb.setup_json_bytes(), Some(before), "setup.json is untouched by a failing run");
    let status = sb.status();
    status.assert_code(4);
    assert!(status.stdout.lines().any(|l| l == "female clip: missing"), "{}", status.context());
    // Recovery.
    sb.setup("fixed").run().assert_code(0);
    sb.assert_status_ready();
}

// [Scenario: Record written last, first run (automated) | spec-narrate-setup Req 6 | FR-010]
#[test]
fn a_failing_first_run_writes_no_setup_json_and_keeps_completed_steps() {
    for fail in ["kokoro_clip:kokoro_failed", "synth:synth_failed", "load:model_load_failed"] {
        let sb = Sb::new();
        let out = sb.setup("fail").env("FAKE_WORKER_FAIL_OP", fail).run();
        out.assert_code(5);
        assert!(
            out.stderr.lines().any(|l| l.starts_with("error[setup-step-failed]: setup failed at step voices: ")),
            "{fail}\n{}",
            out.context()
        );
        assert!(sb.setup_json_bytes().is_none(), "{fail}: setup.json must not exist");
        assert!(sb.join("python-cache").exists() && sb.join("venv/pyvenv.cfg").exists(), "{fail}: completed steps are kept");
        assert!(sb.join("models/chatterbox-turbo/conds.pt").is_file(), "{fail}");
        let status = sb.status();
        status.assert_code(4);
    }
}

// [Requirement 5: reference text, minimum length | spec-narrate-setup | FR-010 | NFR-004]
#[test]
fn kokoro_clips_must_reach_240000_samples() {
    // Boundary: exactly 240000 samples passes (the default), 239999 fails even after the retry.
    let ok = Sb::new();
    ok.setup("ok").env("FAKE_WORKER_CLIP_SAMPLES", "240000").run().assert_code(0);
    assert_eq!(wav_samples(&ok.clip_path("female")), 240_000);

    let sb = Sb::new();
    let out = sb.setup("short").env("FAKE_WORKER_CLIP_SAMPLES", "239999").run();
    out.assert_code(5);
    let line = out
        .stderr
        .lines()
        .find(|l| l.starts_with("error[setup-step-failed]: setup failed at step voices: "))
        .unwrap_or_else(|| panic!("{}", out.context()))
        .to_string();
    assert!(line.contains("female"), "the clip is named: {line}");
    assert!(sb.setup_json_bytes().is_none());
}

// [Automated setup-step tests: samples-per-character arithmetic | spec-narrate-test-strategy | FR-010]
#[test]
fn builtin_clip_uses_the_reference_text_once_when_it_is_long_enough() {
    assert_eq!(REFERENCE_TEXT.chars().count(), 223);
    // 223 x 1077 = 240171 samples is enough at once; 1100 is the suite default.
    for spc in ["1077", "1100"] {
        let sb = Sb::new();
        sb.setup("run1").env("FAKE_WORKER_SAMPLES_PER_CHAR", spc).run().assert_code(0);
        let synth = requests_of(&sb.w_log("run1"), "synth");
        assert_eq!(synth.len(), 1, "spc {spc}");
        assert_eq!(wav_samples(&sb.clip_path("builtin")) as u64, 223 * spc.parse::<u64>().unwrap());
    }
}

// [Automated setup-step tests: doubled reference text | spec-narrate-test-strategy / spec-narrate-setup Req 5 | FR-010]
#[test]
fn a_short_builtin_render_is_repeated_once_with_the_doubled_reference_text() {
    let doubled = format!("{REFERENCE_TEXT} {REFERENCE_TEXT}");
    assert_eq!(doubled.chars().count(), 447);
    // 1076 x 223 = 239948 < 240000 but the doubled text is long enough; 537 x 447 = 240039 is the
    // smallest value that works for the doubled text; 600 is the value in the spec discussion.
    for spc in ["1076", "600", "537"] {
        let sb = Sb::new();
        let out = sb.setup("run1").env("FAKE_WORKER_SAMPLES_PER_CHAR", spc).run();
        out.assert_code(0);
        let synth = requests_of(&sb.w_log("run1"), "synth");
        assert_eq!(synth.len(), 2, "spc {spc}: one render, one re-render");
        assert_eq!(synth[0]["request"]["text"].as_str(), Some(REFERENCE_TEXT));
        assert_eq!(synth[1]["request"]["text"].as_str(), Some(doubled.as_str()));
        for s in &synth {
            let r = &s["request"];
            assert_eq!((r["chunk"].as_u64(), r["total"].as_u64(), r["seed"].as_u64()), (Some(1), Some(1), Some(1)));
        }
        let samples = wav_samples(&sb.clip_path("builtin")) as u64;
        assert_eq!(samples, 447 * spc.parse::<u64>().unwrap(), "spc {spc}");
        assert!(samples >= 240_000, "builtin.wav is written with at least 240000 samples");
        sb.assert_status_ready();
    }
}

// [Requirement 5: still too short after the retry | spec-narrate-setup | FR-010]
#[test]
fn a_builtin_render_that_stays_short_fails_the_voices_step() {
    // 536 x 447 = 239592 < 240000 even with the doubled text.
    let sb = Sb::new();
    let out = sb.setup("run1").env("FAKE_WORKER_SAMPLES_PER_CHAR", "536").run();
    out.assert_code(5);
    let line = out
        .stderr
        .lines()
        .find(|l| l.starts_with("error[setup-step-failed]: setup failed at step voices: "))
        .unwrap_or_else(|| panic!("{}", out.context()))
        .to_string();
    assert!(line.contains("builtin"), "{line}");
    assert!(sb.setup_json_bytes().is_none());
    assert!(!sb.join("voices/builtin.wav").exists(), "a too-short clip is never moved into voices\\");
    assert_eq!(requests_of(&sb.w_log("run1"), "synth").len(), 2);
}

// ---------------------------------------------------------------------------------------
// Environment step
// ---------------------------------------------------------------------------------------

// [Requirement 3: uv failure mapping | spec-narrate-setup | FR-010 | NFR-001 | NFR-007]
#[test]
fn a_failing_uv_sync_reports_the_last_error_line_at_step_environment() {
    let sb = Sb::new();
    let out = sb
        .setup("run1")
        .env("FAKE_UV_SYNC_EXIT", "2")
        .env("FAKE_UV_SYNC_STDERR", "warning: something\nerror: first problem\nerror: last problem")
        .run();
    out.assert_code(5);
    let line = out
        .stderr
        .lines()
        .find(|l| l.starts_with("error[setup-step-failed]: setup failed at step environment: "))
        .unwrap_or_else(|| panic!("{}", out.context()))
        .to_string();
    assert!(line.contains("last problem") && !line.contains("first problem"), "{line}");
    assert!(out.has_stderr_line(&format!("  fix: {FIX_NETWORK}")), "{}", out.context());
    assert!(sb.setup_json_bytes().is_none());
    assert!(sb.join("python-cache").exists(), "step python is kept");
    // uv output goes to logs\setup-last.log, and only with --verbose to stderr as `uv: ` lines.
    assert!(read_lossy(&sb.join("logs/setup-last.log")).contains("last problem"));
    assert!(!out.stderr.lines().any(|l| l.starts_with("uv: ")), "{}", out.context());
    let loud = sb
        .setup("run2")
        .arg("--verbose")
        .env("FAKE_UV_SYNC_EXIT", "2")
        .env("FAKE_UV_SYNC_STDERR", "error: only problem")
        .run();
    loud.assert_code(5);
    assert!(loud.stderr.lines().any(|l| l.starts_with("uv: ") && l.contains("only problem")), "{}", loud.context());
}

// [Requirement 3: Hash mismatch from uv | spec-narrate-setup | NFR-001 | FR-011]
#[test]
fn uv_hash_mismatch_is_reported_as_hash_mismatch() {
    let sb = Sb::new();
    let out = sb
        .setup("run1")
        .env("FAKE_UV_SYNC_EXIT", "2")
        .env("FAKE_UV_SYNC_STDERR", "error: Failed to download `foo==1.0`\n  Caused by: Hash mismatch for `foo==1.0`")
        .run();
    out.assert_code(5);
    assert!(out.stderr.lines().any(|l| l.starts_with("error[hash-mismatch]: ")), "{}", out.context());
    assert!(out.has_stderr_line(&format!("  fix: {FIX_HASH}")), "{}", out.context());
    assert!(sb.setup_json_bytes().is_none());
    assert_eq!(sb.status().code, 4);
}

// [Requirement 3: package component required after sync | spec-narrate-setup | FR-010]
#[test]
fn a_sync_that_leaves_no_package_fails_the_environment_step() {
    let sb = Sb::new();
    let out = sb.setup("run1").env("FAKE_UV_SYNC_NO_PACKAGE", "1").run();
    out.assert_code(5);
    assert!(
        out.stderr.lines().any(|l| l.starts_with("error[setup-step-failed]: setup failed at step environment")),
        "{}",
        out.context()
    );
    assert!(sb.setup_json_bytes().is_none());
}

// [Requirements 2 and 3: repairing a partial setup | spec-narrate-setup | FR-010 | UR-003]
#[test]
fn a_broken_environment_or_python_cache_is_repaired_by_rerunning_setup() {
    let sb = Sb::new();
    sb.complete();
    // Broken venv: python stays offline-complete, the environment is downloaded again.
    fs::remove_file(sb.join("venv/pyvenv.cfg")).unwrap();
    let out = sb.setup("env").run();
    out.assert_code(0);
    assert!(out.has_stderr_line(&step(2, "python", "ok (no network used)")), "{}", out.context());
    assert!(out.has_stderr_line(&step(3, "environment", "ok (downloaded)")), "{}", out.context());
    assert!(sb.join("venv/pyvenv.cfg").exists());

    // Lost python cache: the offline attempt fails, both directories are rebuilt without --offline.
    fs::remove_dir_all(sb.join("python-cache")).unwrap();
    let out = sb.setup("py").run();
    out.assert_code(0);
    assert!(out.has_stderr_line(&step(2, "python", "ok (downloaded)")), "{}", out.context());
    assert!(out.has_stderr_line(&step(3, "environment", "ok (no network used)")), "{}", out.context());
    sb.assert_status_ready();
}

// ---------------------------------------------------------------------------------------
// Child process environment and housekeeping (NFR-002)
// ---------------------------------------------------------------------------------------

// [Child process environment | spec-narrate-setup | NFR-002 | NFR-003]
#[test]
fn uv_and_worker_children_get_redirected_locations_and_a_shared_per_run_temp_directory() {
    let sb = Sb::new();
    let inherited_temp = scratch();
    let out = sb
        .setup("run1")
        .env("TEMP", inherited_temp.path())
        .env("TMP", inherited_temp.path())
        .env("UV_CACHE_DIR", r"C:\bogus-uv-cache")
        .env("UV_INDEX_URL", "https://evil.example.com/simple")
        .env("HF_ENDPOINT", "https://evil.example.com")
        .env("PIP_INDEX_URL", "https://evil.example.com/simple")
        .env("PYTHONPATH", r"C:\bogus-py")
        .env("NARRATE_TEST_BOGUS", "1")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .run();
    out.assert_code(0);
    let d = sb.d();

    // uv children.
    let dumps = uv_env_dumps(&sb.uv_dump("run1"));
    assert!(dumps.len() >= 5, "{} uv invocations recorded", dumps.len());
    let first = &dumps[0];
    assert_eq!(first["args"][0], "--version", "the preflight probe comes first");
    assert!(
        env_names(first).iter().all(|n| !n.to_ascii_uppercase().starts_with("UV_")),
        "inherited UV_ variables are removed even for the probe: {:?}",
        env_names(first)
    );
    assert_eq!(env_get(first, "TEMP").as_deref(), inherited_temp.path().to_str(), "the probe keeps the inherited TEMP");
    assert_eq!(env_get(first, "TMP").as_deref(), inherited_temp.path().to_str());

    let mut run_dirs: Vec<String> = Vec::new();
    for dump in &dumps[1..] {
        let temp = env_get(dump, "TEMP").expect("TEMP for uv steps");
        assert_eq!(env_get(dump, "TMP"), Some(temp.clone()));
        run_dirs.push(temp);
        let expect = |name: &str, value: String| assert_eq!(env_get(dump, name), Some(value), "{name} in {}", dump["args"]);
        expect("UV_CACHE_DIR", d.join("uv-cache").display().to_string());
        expect("UV_PYTHON_INSTALL_DIR", d.join("python").display().to_string());
        expect("UV_PYTHON_CACHE_DIR", d.join("python-cache").display().to_string());
        expect("UV_PROJECT_ENVIRONMENT", d.join("venv").display().to_string());
        expect("UV_NO_CONFIG", "1".to_string());
        assert_eq!(env_get(dump, "UV_INDEX_URL"), None);
        assert_eq!(env_get(dump, "PIP_INDEX_URL"), None);
        assert_eq!(env_get(dump, "PYTHONPATH"), None);
        assert_eq!(env_get(dump, "HF_ENDPOINT"), None);
        assert_eq!(env_get(dump, "NARRATE_TEST_BOGUS"), None);
        assert_eq!(env_get(dump, "HTTPS_PROXY").as_deref(), Some("http://127.0.0.1:9"), "proxy variables pass through");
        let is_sync = dump["args"][0] == "sync";
        let downloads = env_get(dump, "UV_PYTHON_DOWNLOADS");
        if is_sync {
            assert_eq!(downloads.as_deref(), Some("never"), "sync may only use the verified Python");
        } else {
            assert_eq!(downloads, None, "python install may download");
        }
    }

    // Worker children: same run directory, online for downloads, offline for the voices session.
    let worker_dumps: Vec<(u64, Value)> = sessions(&sb.w_log("run1"))
        .iter()
        .map(|(pid, _)| (*pid, read_json(&sb.root.path().join(format!("wenv-run1-{pid}.json")))))
        .collect();
    assert_eq!(worker_dumps.len(), 2);
    for (_, dump) in &worker_dumps {
        let temp = env_get(dump, "TEMP").expect("TEMP for workers");
        assert_eq!(env_get(dump, "TMP"), Some(temp.clone()));
        run_dirs.push(temp);
        assert_eq!(dump["temp_exists"], true, "D\\tmp\\<run> exists while the child runs");
        assert_eq!(env_get(dump, "HF_HOME"), Some(d.join("hf-home").display().to_string()));
        assert_eq!(env_get(dump, "TORCH_HOME"), Some(d.join("torch-home").display().to_string()));
        assert_eq!(env_get(dump, "UV_INDEX_URL"), None);
        assert_eq!(env_get(dump, "HF_ENDPOINT"), None);
        assert_eq!(env_get(dump, "PYTHONPATH"), None);
        assert_eq!(env_get(dump, "NARRATE_TEST_BOGUS"), None);
        assert!(same_path(dump["cwd"].as_str().unwrap(), &d), "worker working directory is the data directory");
    }
    assert_eq!(env_get(&worker_dumps[0].1, "HF_HUB_OFFLINE"), None, "download sessions are online");
    assert_eq!(env_get(&worker_dumps[0].1, "TRANSFORMERS_OFFLINE"), None);
    assert_eq!(env_get(&worker_dumps[1].1, "HF_HUB_OFFLINE").as_deref(), Some("1"));
    assert_eq!(env_get(&worker_dumps[1].1, "TRANSFORMERS_OFFLINE").as_deref(), Some("1"));

    // One <narrate pid>-<unix ms> directory under D\tmp for every child, gone afterwards.
    run_dirs.dedup();
    run_dirs.sort();
    run_dirs.dedup();
    assert_eq!(run_dirs.len(), 1, "all children share one run directory: {run_dirs:?}");
    let run_dir = PathBuf::from(&run_dirs[0]);
    assert!(same_path(&run_dir.parent().unwrap().display().to_string(), &d.join("tmp")), "{}", run_dir.display());
    let name = run_dir.file_name().unwrap().to_string_lossy().into_owned();
    let (pid, millis) = name.split_once('-').unwrap_or_else(|| panic!("<pid>-<unix ms>: {name}"));
    assert!(pid.bytes().all(|b| b.is_ascii_digit()) && millis.bytes().all(|b| b.is_ascii_digit()) && !pid.is_empty() && !millis.is_empty());
    assert!(!run_dir.exists());
    assert!(!sb.join("tmp").exists());
}

// [Child process environment: stale entries | spec-narrate-setup | NFR-002 | RULE-004]
#[test]
fn setup_removes_stale_tmp_entries_and_old_trimmed_clips_and_keeps_everything_else() {
    let sb = Sb::new();
    let old_tmp = write_bytes(sb.data.path(), "tmp/old.bin", b"stale");
    let new_tmp = write_bytes(sb.data.path(), "tmp/new.bin", b"recent");
    age_file(&old_tmp, 25);
    let old_clip = write_bytes(sb.data.path(), "staging/voice-1.wav", b"old clip");
    let young_clip = write_bytes(sb.data.path(), "staging/voice-2.wav", b"young clip");
    let other = write_bytes(sb.data.path(), "staging/keep.txt", b"keep");
    age_file(&old_clip, 2);
    age_file(&other, 2);
    sb.setup("run1").run().assert_code(0);
    assert!(!old_tmp.exists(), "tmp entries older than 24 hours are removed");
    assert!(new_tmp.exists(), "recent entries stay");
    assert!(!old_clip.exists(), "staging\\voice-*.wav older than 1 hour is removed after preflight");
    assert!(young_clip.exists());
    assert!(other.exists());
}

// ---------------------------------------------------------------------------------------
// Interruption (Requirement 9)
// ---------------------------------------------------------------------------------------

// [Scenario: Interrupted environment step (automated) | spec-narrate-setup Req 9 | FR-010 | NFR-008]
#[test]
fn ctrl_break_during_uv_sync_exits_130_stops_uv_keeps_progress_and_releases_the_lock() {
    let sb = Sb::new();
    let kit = scratch();
    let mut run = sb
        .setup("run1")
        .env("FAKE_UV_SYNC_SLEEP_MS", "30000")
        .arg("--verbose")
        .spawn_group(kit.path());
    let uv_log = sb.uv_log("run1");
    let saw_sync = wait_until(Duration::from_secs(60), || !uv_calls(&uv_log, "sync").is_empty());
    assert!(saw_sync, "the fake uv never logged a sync invocation\n{}", run.stderr());
    assert!(
        send_ctrl_break(run.pid()),
        "narrate-test-signal could not deliver CTRL_BREAK_EVENT; run the tests from a console session"
    );
    let code = run.wait(Duration::from_secs(10));
    let stderr = run.stderr();
    assert_eq!(code, Some(130), "setup must exit 130 within 10 s\n{stderr}");
    assert!(stderr.lines().any(|l| l == "error[setup-interrupted]: setup was interrupted"), "{stderr}");
    assert!(stderr.lines().any(|l| l == "  fix: run narrate setup again to continue"), "{stderr}");
    assert!(!stderr.contains("error[setup-step-failed]"), "an interrupt is not a step failure\n{stderr}");
    assert!(sb.setup_json_bytes().is_none(), "setup.json must not exist");

    // Every uv child that setup started is gone.
    let pids: Vec<u64> = stderr.lines().filter_map(|l| l.strip_prefix("verbose: uv pid ")).filter_map(|p| p.trim().parse().ok()).collect();
    assert!(!pids.is_empty());
    for pid in pids {
        assert!(wait_until(Duration::from_secs(10), || !pid_alive(pid)), "uv pid {pid} is still running");
    }

    // Completed steps are kept and the lock is released: a second run resumes and finishes.
    assert!(sb.join("python-cache").exists() && sb.join("python").exists());
    let out = sb.setup("run2").run();
    out.assert_code(0);
    assert!(out.has_stderr_line(&step(2, "python", "ok (no network used)")), "{}", out.context());
    assert_eq!(out.stderr_lines().last().copied(), Some("setup: ready"));
    sb.assert_status_ready();
}
