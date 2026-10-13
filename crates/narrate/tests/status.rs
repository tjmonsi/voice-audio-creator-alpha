//! `narrate status`, readiness, data directory override and pin set identity
//! (FR-012, FR-014, FR-015, FR-017, UR-006).
//!
//! Sources: specs/spec-narrate-setup.md (Requirements 7 and 8, Pin set identifier),
//! specs/spec-narrate-architecture.md (Data directory resolution), docs/requirements.md FR-014,
//! FR-015, FR-017. Ready directories are built by the recipe of
//! specs/spec-narrate-test-strategy.md. No implementation was read.

mod common;

use common::*;
use std::fs;
use std::path::{Path, PathBuf};

const NOT_READY: &str = "status: not ready (run narrate setup)";

fn tree(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = walk(dir)
        .into_iter()
        .map(|p| {
            let rel = p.strip_prefix(dir).unwrap().display().to_string();
            if p.is_dir() {
                format!("{rel}\\")
            } else {
                format!("{rel} ({} bytes)", fs::metadata(&p).map(|m| m.len()).unwrap_or(0))
            }
        })
        .collect();
    v.sort();
    v
}

// ---------------------------------------------------------------------------------------
// Empty and missing directories
// ---------------------------------------------------------------------------------------

// [Scenario: Empty scratch directory (FR-017 override) | spec-narrate-setup Req 8 | FR-014 | FR-017 | NFR-008]
#[test]
fn empty_scratch_directory_reports_everything_missing_and_exits_4() {
    let data = scratch();
    let out = Nar::new(data.path()).arg("status").run();
    out.assert_code(4);
    let lines = out.stdout_lines();
    assert_eq!(lines.len(), 12, "the report has exactly twelve lines\n{}", out.context());
    assert_eq!(lines[0], "narrate 0.1.0 (test hooks enabled)");
    assert_eq!(lines[1], format!("data directory: {}", abs(data.path()).display()));
    assert_eq!(lines[2], "environment: missing");
    assert_eq!(lines[3], "package: missing");
    assert!(lines[4].starts_with("turbo weights: missing (") && lines[4].ends_with(')'), "{}", lines[4]);
    for f in TURBO_FILES {
        assert!(lines[4].contains(f), "turbo list names {f}: {}", lines[4]);
    }
    assert!(lines[5].starts_with("kokoro weights: missing (") && lines[5].ends_with(')'), "{}", lines[5]);
    for f in ["kokoro-v1_0.pth", "config.json", "af_heart.pt", "am_michael.pt"] {
        assert!(lines[5].contains(f), "kokoro list names {f}: {}", lines[5]);
    }
    assert_eq!(lines[6], "female clip: missing");
    assert_eq!(lines[7], "male clip: missing");
    assert_eq!(lines[8], "builtin clip: missing");
    // matches ^pin set: missing \(binary [0-9a-f]{64}, recorded none\)$
    let id = parse_missing_pin_id(&out.stdout);
    assert_eq!(lines[9], format!("pin set: missing (binary {id}, recorded none)"));
    assert_eq!(lines[10], "cuda: unknown (environment missing)");
    assert_eq!(lines[11], NOT_READY);
    assert!(is_empty_dir(data.path()), "status must not create anything in the scratch directory");
}

// [Scenario: Missing directory not created | spec-narrate-architecture | FR-017 | FR-014]
#[test]
fn nonexistent_data_directory_is_reported_not_ready_and_not_created() {
    let base = scratch();
    let missing = base.path().join("does-not-exist");
    let out = Nar::new(&missing).arg("status").run();
    out.assert_code(4);
    let want = format!("data directory: {}", abs(&missing).display());
    assert!(out.stdout.lines().any(|l| l == want), "{}", out.context());
    assert_eq!(out.stdout_lines().last().copied(), Some(NOT_READY));
    assert!(!missing.exists(), "status must not create the data directory");
}

// [Required data: NARRATE_DATA_DIR relative values | spec-narrate-cli | FR-017]
#[test]
fn relative_data_directory_is_resolved_against_the_current_directory() {
    let cwd = scratch();
    let out = Nar::new(Path::new("rel-data")).arg("status").dir(cwd.path()).run();
    out.assert_code(4);
    let expected = abs(&cwd.path().join("rel-data"));
    let want = format!("data directory: {}", expected.display());
    assert!(out.stdout.lines().any(|l| l == want), "{}", out.context());
    assert!(!expected.exists());
    assert!(is_empty_dir(cwd.path()));
}

// [Scenario: Override honored | spec-narrate-architecture | FR-017]
#[test]
fn override_leaves_the_default_data_directory_untouched() {
    // Read-only snapshot of %LOCALAPPDATA%\narrate: existence and top-level entries.
    let default_dir = std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("narrate"));
    let snapshot = |d: &Option<PathBuf>| -> Option<Vec<String>> {
        d.as_ref().and_then(|p| if p.exists() { Some(list_dir(p)) } else { None })
    };
    let before = snapshot(&default_dir);
    let data = scratch();
    let out = Nar::new(data.path()).arg("status").run();
    out.assert_code(4);
    let want = format!("data directory: {}", abs(data.path()).display());
    assert!(out.stdout.lines().any(|l| l == want));
    assert_eq!(snapshot(&default_dir), before, "%LOCALAPPDATA%\\narrate keeps its existence and top-level entries");
}

// ---------------------------------------------------------------------------------------
// Ready directory
// ---------------------------------------------------------------------------------------

// [Scenario: Fabricated directory accepted + Ready | spec-narrate-test-strategy / spec-narrate-setup Req 8 | FR-014 | FR-017]
#[test]
fn fabricated_ready_directory_reports_ready_with_the_fake_worker() {
    let ready = Ready::new();
    let kit = scratch();
    let wlog = kit.path().join("worker.log");
    let before = tree(ready.path());
    let out = ready.nar().log(&wlog).arg("status").run();
    out.assert_code(0);
    let lines = out.stdout_lines();
    assert_eq!(lines.len(), 12, "{}", out.context());
    assert_eq!(lines[0], "narrate 0.1.0 (test hooks enabled)");
    assert_eq!(lines[1], format!("data directory: {}", ready.d().display()));
    assert_eq!(lines[2], "environment: present");
    assert_eq!(lines[3], "package: present (chatterbox-tts 0.1.7)");
    assert_eq!(lines[4], "turbo weights: present");
    assert_eq!(lines[5], "kokoro weights: present");
    assert_eq!(lines[6], "female clip: present");
    assert_eq!(lines[7], "male clip: present");
    assert_eq!(lines[8], "builtin clip: present");
    assert_eq!(lines[9], format!("pin set: match (binary {id}, recorded {id})", id = ready.id));
    assert!(lines[10].starts_with("cuda: "), "{}", lines[10]);
    assert_eq!(lines[11], "status: ready");

    // The probe session is hello, probe, shutdown, and status leaves no trace in the directory.
    assert_eq!(op_sequence(&wlog), vec!["hello", "probe", "shutdown"]);
    assert_eq!(tree(ready.path()), before, "status changes nothing (tmp, logs and worker\\ stay absent)");
    assert!(!ready.path().join("logs").exists(), "status writes no log file");
    assert!(!ready.path().join("worker").exists(), "NARRATE_WORKER_CMD means no worker script is materialized");
}

// [Scenario: CUDA reported from the worker | spec-narrate-setup Req 8 | FR-014]
#[test]
fn cuda_line_comes_from_the_probe_reply() {
    let ready = Ready::new();
    let out = ready.nar().arg("status").run();
    out.assert_code(0);
    assert!(out.stdout.lines().any(|l| l == "cuda: available (Fake GPU, torch 2.6.0+cu124)"), "{}", out.context());

    let out = ready.nar().env("FAKE_WORKER_PROBE_CUDA", "0").arg("status").run();
    out.assert_code(0);
    assert!(out.stdout.lines().any(|l| l == "cuda: not available (torch 2.6.0+cu124)"), "{}", out.context());
}

// [Error handling: Probe fails in status | spec-narrate-setup | FR-014]
#[test]
fn failed_probe_is_reported_without_affecting_readiness() {
    let ready = Ready::new();
    let out = ready.nar().env("FAKE_WORKER_FAIL_OP", "probe:probe_failed").arg("status").run();
    out.assert_code(0);
    assert!(out.stdout.lines().any(|l| l == "cuda: unknown (probe failed)"), "{}", out.context());
    assert_eq!(out.stdout_lines().last().copied(), Some("status: ready"));
}

// [Requirement 8: probe worker environment | spec-narrate-setup | NFR-002]
#[test]
fn status_probe_worker_gets_the_per_run_temp_directory_and_it_is_removed() {
    let ready = Ready::new();
    let kit = scratch();
    let dump = kit.path().join("env.json");
    ready.nar().env("FAKE_WORKER_ENV_DUMP", &dump).arg("status").run().assert_code(0);
    let d = read_json(&dump);
    let temp = PathBuf::from(env_get(&d, "TEMP").expect("TEMP"));
    assert_eq!(env_get(&d, "TMP").as_deref(), Some(temp.to_str().unwrap()));
    assert!(same_path(&temp.parent().unwrap().display().to_string(), &ready.d().join("tmp")), "{}", temp.display());
    assert_eq!(d["temp_exists"], true);
    assert!(!temp.exists());
    assert!(!ready.path().join("tmp").exists(), "D\\tmp is removed when left empty");
    assert_eq!(env_get(&d, "HF_HUB_OFFLINE").as_deref(), Some("1"));
}

// ---------------------------------------------------------------------------------------
// Not ready
// ---------------------------------------------------------------------------------------

// [Scenario: Not ready (weights missing) | spec-narrate-setup Req 8 | FR-014 | FR-015]
#[test]
fn missing_turbo_weight_is_named_and_status_exits_4() {
    let ready = Ready::new();
    ready.remove("models/chatterbox-turbo/t3_turbo_v1.safetensors");
    let out = ready.nar().arg("status").run();
    out.assert_code(4);
    assert!(out.stdout.lines().any(|l| l == "turbo weights: missing (t3_turbo_v1.safetensors)"), "{}", out.context());
    assert_eq!(out.stdout_lines().last().copied(), Some(NOT_READY));
}

// [Scenario: Not-ready report | spec-narrate-architecture | FR-014]
#[test]
fn missing_conds_pt_is_reported_as_the_turbo_weight_conds_pt() {
    let ready = Ready::new();
    ready.remove("models/chatterbox-turbo/conds.pt");
    let out = ready.nar().arg("status").run();
    out.assert_code(4);
    assert!(out.stdout.contains("turbo weights: missing (conds.pt)"), "{}", out.context());
    assert_eq!(out.stdout_lines().last().copied(), Some(NOT_READY));
}

// [Requirement 8: several missing files | spec-narrate-setup | FR-014]
#[test]
fn several_missing_files_are_all_listed() {
    let ready = Ready::new();
    ready.remove("models/chatterbox-turbo/vocab.json");
    ready.remove("models/chatterbox-turbo/merges.txt");
    ready.remove("models/kokoro-82m/config.json");
    ready.remove("models/kokoro-82m/voices/am_michael.pt");
    let out = ready.nar().arg("status").run();
    out.assert_code(4);
    let turbo = out.stdout.lines().find(|l| l.starts_with("turbo weights: missing (")).expect("turbo line").to_string();
    assert!(turbo.contains("vocab.json") && turbo.contains("merges.txt"), "{turbo}");
    let kokoro = out.stdout.lines().find(|l| l.starts_with("kokoro weights: missing (")).expect("kokoro line").to_string();
    assert!(kokoro.contains("config.json") && kokoro.contains("am_michael.pt"), "{kokoro}");
    assert!(!kokoro.contains("af_heart.pt") && !kokoro.contains("kokoro-v1_0.pth"), "{kokoro}");
}

// [Requirement 7: each clip is its own component | spec-narrate-setup | FR-014 | FR-008]
#[test]
fn each_missing_clip_is_reported_on_its_own_line() {
    for (rel, line) in [
        ("voices/female.wav", "female clip: missing"),
        ("voices/male.wav", "male clip: missing"),
        ("voices/builtin.wav", "builtin clip: missing"),
    ] {
        let ready = Ready::new();
        ready.remove(rel);
        let out = ready.nar().arg("status").run();
        out.assert_code(4);
        assert!(out.stdout.lines().any(|l| l == line), "{rel}\n{}", out.context());
        assert_eq!(out.stdout_lines().iter().filter(|l| l.ends_with(": missing")).count(), 1, "only {rel} is missing");
    }
}

// [Requirement 7: environment component | spec-narrate-setup | FR-014]
#[test]
fn missing_interpreter_or_pyvenv_cfg_means_environment_missing_and_no_probe_worker() {
    for rel in ["venv/Scripts/python.exe", "venv/pyvenv.cfg"] {
        let ready = Ready::new();
        ready.remove(rel);
        let kit = scratch();
        let wlog = kit.path().join("worker.log");
        let out = ready.nar().log(&wlog).arg("status").run();
        out.assert_code(4);
        assert!(out.stdout.lines().any(|l| l == "environment: missing"), "{rel}\n{}", out.context());
        assert!(out.stdout.lines().any(|l| l == "cuda: unknown (environment missing)"), "{rel}\n{}", out.context());
        assert!(!wlog.exists(), "the probe worker starts only when the environment is present");
    }
}

// [Requirement 7: package component | spec-narrate-setup | FR-014]
#[test]
fn missing_package_metadata_is_reported_but_the_probe_still_runs() {
    let ready = Ready::new();
    ready.remove("venv/Lib/site-packages/chatterbox_tts-0.1.7.dist-info/METADATA");
    let out = ready.nar().arg("status").run();
    out.assert_code(4);
    assert!(out.stdout.lines().any(|l| l == "package: missing"), "{}", out.context());
    assert!(out.stdout.lines().any(|l| l == "environment: present"), "{}", out.context());
    assert!(out.stdout.lines().any(|l| l.starts_with("cuda: available")), "{}", out.context());
}

// ---------------------------------------------------------------------------------------
// Pin set identity (FR-017)
// ---------------------------------------------------------------------------------------

// [Scenario: Pin-set mismatch (FR-017) | spec-narrate-setup Req 8 | FR-017 | FR-014]
#[test]
fn different_recorded_pin_set_is_a_mismatch() {
    let ready = Ready::new();
    let zeros = "0".repeat(64);
    fs::write(ready.path().join("setup.json"), setup_json(&zeros)).unwrap();
    let out = ready.nar().arg("status").run();
    out.assert_code(4);
    let want = format!("pin set: mismatch (binary {}, recorded {zeros})", ready.id);
    assert!(out.stdout.lines().any(|l| l == want), "{}", out.context());
    assert!(out.stdout.contains("pin set: mismatch (binary "));
    assert_eq!(out.stdout_lines().last().copied(), Some(NOT_READY));
}

// [Requirement 7: setup record | spec-narrate-setup | FR-017 | FR-015]
#[test]
fn missing_or_unparseable_setup_json_is_not_a_match() {
    let ready = Ready::new();
    fs::remove_file(ready.path().join("setup.json")).unwrap();
    let out = ready.nar().arg("status").run();
    out.assert_code(4);
    let want = format!("pin set: missing (binary {}, recorded none)", ready.id);
    assert!(out.stdout.lines().any(|l| l == want), "{}", out.context());

    fs::write(ready.path().join("setup.json"), "{ this is not json").unwrap();
    let out = ready.nar().arg("status").run();
    out.assert_code(4);
    let pin_line = out.stdout.lines().find(|l| l.starts_with("pin set: ")).expect("pin set line").to_string();
    assert!(!pin_line.starts_with("pin set: match"), "{pin_line}");
    assert_eq!(out.stdout_lines().last().copied(), Some(NOT_READY));
}

// [Requirement 7: unknown fields ignored | spec-narrate-setup | FR-017]
#[test]
fn setup_record_with_extra_fields_still_matches() {
    let ready = Ready::new();
    fs::write(
        ready.path().join("setup.json"),
        format!(
            "{{\"format\":1,\"pin_set_id\":\"{}\",\"narrate_version\":\"0.1.0\",\"completed_unix\":1760300000,\"clips\":{{\"female\":{{\"sha256\":\"{}\",\"samples\":312000}}}},\"extra\":null}}",
            ready.id,
            "a".repeat(64)
        ),
    )
    .unwrap();
    let out = ready.nar().arg("status").run();
    out.assert_code(0);
    assert!(out.stdout.lines().any(|l| l.starts_with("pin set: match")), "{}", out.context());
}

// [Pin set identifier | spec-narrate-setup | FR-017]
#[test]
fn binary_pin_set_id_follows_the_documented_formula_over_the_embedded_files() {
    let pins = fs::read(real_pins_path()).expect("pins.json");
    let pyproject = fs::read(real_pyproject_path()).expect("pyproject.toml");
    let lock = fs::read(real_lock_path()).expect("uv.lock");
    let expected = compute_pin_set_id(&pins, &pyproject, &lock);
    assert_eq!(binary_pin_set_id(), expected, "pin_set_id = sha256(narrate-pin-set-v1 + hashes of pins, pyproject, lock)");
}

// [Pin set identifier: test overrides are included | spec-narrate-setup | FR-017]
#[test]
fn pins_and_lock_overrides_change_the_pin_set_id_by_the_same_formula() {
    let work = scratch();
    let pyproject = fs::read(real_pyproject_path()).unwrap();
    let real_pins = fs::read(real_pins_path()).unwrap();
    let real_lock = fs::read(real_lock_path()).unwrap();

    // Pins override: the committed bytes plus one trailing newline (still valid JSON).
    let mut pins_bytes = real_pins.clone();
    pins_bytes.push(b'\n');
    let pins_copy = write_bytes(work.path(), "pins-copy.json", &pins_bytes);
    let data = scratch();
    let out = Nar::new(data.path()).arg("status").env("NARRATE_TEST_PINS_FILE", &pins_copy).run();
    let id = parse_missing_pin_id(&out.stdout);
    assert_eq!(id, compute_pin_set_id(&pins_bytes, &pyproject, &real_lock));
    assert_ne!(id, binary_pin_set_id());

    // Lock override: the committed bytes plus a trailing comment line.
    let mut lock_bytes = real_lock.clone();
    lock_bytes.extend_from_slice(b"\n# changed for the test\n");
    let lock_copy = write_bytes(work.path(), "lock-copy.lock", &lock_bytes);
    let data2 = scratch();
    let out = Nar::new(data2.path()).arg("status").env("NARRATE_TEST_LOCK_FILE", &lock_copy).run();
    let id2 = parse_missing_pin_id(&out.stdout);
    assert_eq!(id2, compute_pin_set_id(&real_pins, &pyproject, &lock_bytes));
    assert_ne!(id2, id);
}

// [Scenario: Dev build reuses setup (same binary, repeated runs) | spec-narrate-setup Req 7 | FR-017]
#[test]
fn the_pin_set_id_is_stable_across_runs_of_the_same_binary() {
    let a = scratch();
    let b = scratch();
    let first = parse_missing_pin_id(&Nar::new(a.path()).arg("status").run().stdout);
    let second = parse_missing_pin_id(&Nar::new(b.path()).arg("status").run().stdout);
    assert_eq!(first, second);
}

// ---------------------------------------------------------------------------------------
// Verbose and streams
// ---------------------------------------------------------------------------------------

// [Stream discipline: the report is stdout | spec-narrate-cli | FR-024 | FR-014]
#[test]
fn verbose_does_not_change_the_status_report_on_stdout() {
    let ready = Ready::new();
    let plain = ready.nar().arg("status").run();
    let loud = ready.nar().arg("status").arg("--verbose").run();
    plain.assert_code(0);
    loud.assert_code(0);
    assert_eq!(plain.stdout, loud.stdout);
}

// ---------------------------------------------------------------------------------------
// FR-012: weights missing while offline
// ---------------------------------------------------------------------------------------

// [Scenario: Weights missing while offline | docs/requirements FR-012 | FR-012 | FR-015]
#[test]
fn missing_weights_with_closed_proxies_advise_setup_and_attempt_no_download() {
    let ready = Ready::new();
    ready.remove("models/chatterbox-turbo/t3_turbo_v1.safetensors");
    let work = scratch();
    let script = write_text(work.path(), "script.txt", "Hello there.");
    let kit = scratch();
    let wlog = kit.path().join("worker.log");
    let out = ready
        .nar()
        .log(&wlog)
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .arg(&script)
        .run();
    out.assert_error(
        4,
        "not-set-up",
        "setup is incomplete: turbo weights missing (t3_turbo_v1.safetensors)",
        "run narrate setup",
    );
    assert!(!wlog.exists(), "no worker, hence no download attempt");
    assert!(wav_or_tmp_files(work.path()).is_empty());
}
