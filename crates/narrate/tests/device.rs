//! Device selection, CPU fallback warning, CUDA error, model-load errors and the watermark
//! hook (FR-013, FR-018, UR-004).
//!
//! Sources: specs/spec-narrate-generation.md (Requirement 8), specs/spec-narrate-worker.md
//! (Requirements 4, 9; protocol schemas), specs/spec-narrate-cli.md (line and error catalogs).
//! The fake worker's `load` reply is chosen with `FAKE_WORKER_DEVICE`. No implementation was
//! read.

mod common;

use common::*;
use serde_json::Value;
use std::path::Path;

fn script(dir: &Path) -> std::path::PathBuf {
    write_text(dir, "script.txt", "Hello there.")
}

fn load_request(log: &Path) -> Value {
    let v = requests_of(log, "load");
    assert_eq!(v.len(), 1, "exactly one load request: {v:?}");
    v.into_iter().next().unwrap()
}

const FALLBACK_WARNING: &str = "warning: CUDA is not available; falling back to CPU (generation will be slower)";

// [Scenario: Auto with GPU | spec-narrate-generation Req 8 | FR-013 | UR-004]
#[test]
fn auto_with_a_gpu_prints_the_cuda_device_and_no_fallback_warning() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());
    let out = ready
        .nar()
        .env("FAKE_WORKER_DEVICE", "cuda")
        .env("FAKE_WORKER_DEVICE_NAME", "NVIDIA GeForce RTX 3050")
        .arg(&s)
        .args(["--device", "auto"])
        .run();
    out.assert_code(0);
    assert!(out.has_stderr_line("device: cuda (NVIDIA GeForce RTX 3050)"), "{}", out.context());
    out.assert_stderr_lacks("falling back");
}

// [Scenario: Auto with GPU | spec-narrate-generation Req 8 | FR-013]
#[test]
fn device_line_shows_whatever_name_the_worker_reports() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());
    let out = ready.nar().env("FAKE_WORKER_DEVICE_NAME", "Fake GPU 9000").arg(&s).run();
    out.assert_code(0);
    assert!(out.has_stderr_line("device: cuda (Fake GPU 9000)"), "{}", out.context());
}

// [Scenario: Auto without GPU | spec-narrate-generation Req 8 | FR-013 | UR-004]
#[test]
fn auto_without_a_gpu_warns_then_reports_cpu_and_succeeds() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());
    let out = ready.nar().env("FAKE_WORKER_DEVICE", "cpu-fallback").arg(&s).args(["--device", "auto"]).run();
    out.assert_code(0);
    let warn_at = out.stderr_line_index(FALLBACK_WARNING).unwrap_or_else(|| panic!("{}", out.context()));
    let device_at = out.stderr_line_index("device: cpu").unwrap_or_else(|| panic!("{}", out.context()));
    assert!(warn_at < device_at, "warning comes before the device line");
    assert_eq!(out.count_stderr_line(FALLBACK_WARNING), 1);
    let wav = work.path().join("script.wav");
    assert_output_wav_format(&wav);
}

// [Scenario: Auto without GPU, default device | spec-narrate-generation Req 8 | FR-013]
#[test]
fn default_device_is_auto_and_falls_back_with_a_warning() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let s = script(work.path());
    let out = ready.nar().log(&log).env("FAKE_WORKER_DEVICE", "cpu-fallback").arg(&s).run();
    out.assert_code(0);
    assert!(out.has_stderr_line(FALLBACK_WARNING));
    assert_eq!(load_request(&log)["request"]["device"], "auto");
}

// [Requirement 8: explicit CPU | spec-narrate-generation | FR-013]
#[test]
fn explicit_cpu_prints_cpu_without_a_warning() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let s = script(work.path());
    let out = ready.nar().log(&log).env("FAKE_WORKER_DEVICE", "cpu").arg(&s).args(["--device", "cpu"]).run();
    out.assert_code(0);
    assert!(out.has_stderr_line("device: cpu"), "{}", out.context());
    out.assert_stderr_lacks("falling back");
    assert_eq!(load_request(&log)["request"]["device"], "cpu");
}

// [Requirement 8: the load request | spec-narrate-generation | FR-013 | UR-004]
#[test]
fn load_request_carries_the_model_directory_and_the_requested_device() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());
    for device in ["auto", "cuda", "cpu"] {
        let log = work.path().join(format!("{device}.log"));
        ready.nar().log(&log).arg(&s).args(["--device", device]).run().assert_code(0);
        let load = load_request(&log);
        assert_eq!(load["request"]["device"], device);
        let model_dir = load["request"]["model_dir"].as_str().unwrap().to_string();
        assert!(
            same_path(&model_dir, &ready.d().join("models").join("chatterbox-turbo")),
            "model_dir {model_dir}"
        );
    }
}

// [Requirement 8: ordering | spec-narrate-generation | FR-013]
#[test]
fn device_line_is_printed_once_after_the_model_loads_and_before_any_chunk() {
    let ready = Ready::new();
    let work = scratch();
    let s = write_text(work.path(), "two.txt", "One.\n\nTwo.");
    let out = ready.nar().arg(&s).run();
    out.assert_code(0);
    let device_lines: Vec<usize> = out.stderr.lines().enumerate().filter(|(_, l)| l.starts_with("device:")).map(|(i, _)| i).collect();
    assert_eq!(device_lines.len(), 1, "{}", out.context());
    let first_chunk = out.stderr.lines().position(|l| l.starts_with("chunk 1/")).unwrap();
    assert!(device_lines[0] < first_chunk);
}

// [Scenario: CUDA forced but unavailable | spec-narrate-generation Req 8 | FR-013 | UR-004 | NFR-007]
#[test]
fn forced_cuda_without_cuda_exits_1_and_suggests_cpu() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());
    let out = ready.nar().env("FAKE_WORKER_DEVICE", "cuda-unavailable").arg(&s).args(["--device", "cuda"]).run();
    out.assert_error(
        1,
        "cuda-unavailable",
        "CUDA is not available on this machine",
        "use --device auto or --device cpu, or check the NVIDIA driver",
    );
    out.assert_stderr_contains("--device cpu");
    assert!(wav_or_tmp_files(work.path()).is_empty(), "no output file");
}

// [Scenario: Forced CUDA with a GPU | spec-narrate-generation Req 8 | FR-013]
#[test]
fn forced_cuda_with_a_gpu_succeeds() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());
    let out = ready.nar().arg(&s).args(["--device", "cuda"]).run();
    out.assert_code(0);
    assert!(out.has_stderr_line("device: cuda (NVIDIA GeForce RTX 3050)"), "{}", out.context());
}

// [Requirement 8: model_load_failed | spec-narrate-generation | FR-013 | NFR-007]
#[test]
fn model_load_failure_maps_to_model_load() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let s = script(work.path());
    let out = ready.nar().log(&log).env("FAKE_WORKER_FAIL_OP", "load:model_load_failed").arg(&s).run();
    out.assert_error_prefix(
        1,
        "model-load",
        "could not load the Chatterbox-Turbo model: ",
        "run narrate setup to repair the model files",
    );
    assert!(requests_of(&log, "synth").is_empty(), "no chunk may be requested after a failed load");
    assert!(wav_or_tmp_files(work.path()).is_empty());
}

// [Scenario: Watermarker unavailable | spec-narrate-generation Req 8 | FR-018 | NFR-007]
#[test]
fn watermark_hook_fails_the_run_and_writes_no_audio() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let s = script(work.path());
    let out = ready.nar().log(&log).env("NARRATE_TEST_WATERMARK_FAIL", "1").arg(&s).run();
    out.assert_error(
        1,
        "watermark-unavailable",
        "the PerTh watermarker could not be loaded; narrate does not write unwatermarked audio",
        "run narrate setup to repair the environment",
    );
    assert!(wav_or_tmp_files(work.path()).is_empty(), "no .wav or .tmp file may exist");
    assert!(requests_of(&log, "synth").is_empty(), "no audio is ever produced by this session");
    assert_eq!(requests_of(&log, "load").len(), 1);
}

// [Requirement 8: watermarker_unavailable from the worker | spec-narrate-generation | FR-018]
#[test]
fn watermarker_unavailable_error_reply_maps_to_watermark_unavailable() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());
    let out = ready.nar().env("FAKE_WORKER_FAIL_OP", "load:watermarker_unavailable").arg(&s).run();
    out.assert_code(1);
    out.assert_stderr_contains("error[watermark-unavailable]");
    assert!(wav_or_tmp_files(work.path()).is_empty());
}

// [Requirement 8: hook only fires when set to 1 | spec-narrate-cli | FR-018]
#[test]
fn watermark_hook_is_inert_when_unset() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());
    ready.nar().arg(&s).run().assert_code(0);
    assert!(work.path().join("script.wav").is_file());
}
