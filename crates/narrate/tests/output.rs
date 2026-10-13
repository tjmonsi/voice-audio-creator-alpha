//! Output path rules, overwrite behavior, permission failures and the WAV size limit
//! (FR-006, NFR-005, UR-007).
//!
//! Sources: specs/spec-narrate-generation.md (Requirements 2, 11, 12), specs/spec-narrate-cli.md
//! (error catalog, path rule), docs/requirements.md FR-006. No implementation was read.

mod common;

use common::*;
use std::fs;
use std::path::{Path, PathBuf};

fn stdout_is_path(out: &Out, path: &Path) -> bool {
    let p = path.display().to_string();
    out.stdout == format!("{p}\r\n") || out.stdout == format!("{p}\n")
}

struct Unlock(PathBuf);

impl Drop for Unlock {
    fn drop(&mut self) {
        if let Ok(meta) = fs::metadata(&self.0) {
            let mut perms = meta.permissions();
            perms.set_readonly(false);
            let _ = fs::set_permissions(&self.0, perms);
        }
    }
}

// ---------------------------------------------------------------------------------------
// Default output and overrides
// ---------------------------------------------------------------------------------------

// [Scenario: Default output | spec-narrate-generation Req 2 | FR-006 | UR-007]
#[test]
fn default_output_is_the_input_path_with_a_wav_extension() {
    let ready = Ready::new();
    let work = scratch();
    let script = write_text(work.path(), "intro.md", "# Intro\n\nHello there.");
    let out = ready.nar().arg(&script).run();
    out.assert_code(0);
    let wav = abs(&work.path().join("intro.wav"));
    assert!(wav.is_file());
    assert!(stdout_is_path(&out, &wav), "stdout must be exactly the output path\n{}", out.context());
    assert_output_wav_format(&wav);
}

// [Scenario: Default output | spec-narrate-generation Req 2 | FR-006 | UR-007]
#[test]
fn default_output_name_replaces_only_the_last_extension_in_any_case() {
    let ready = Ready::new();
    let work = scratch();
    // (input name, expected output name)
    for (input, output) in [("a.b.txt", "a.b.wav"), ("INTRO.MD", "INTRO.wav"), ("x.markdown", "x.wav"), ("Mixed.Txt", "Mixed.wav")] {
        let script = write_text(work.path(), input, "Hello there.");
        let out = ready.nar().arg(&script).run();
        out.assert_code(0);
        let wav = abs(&work.path().join(output));
        assert!(wav.is_file(), "{input} -> {output}");
        assert!(stdout_is_path(&out, &wav), "{input}: {}", out.context());
    }
}

// [Grammar: relative paths resolve against the current directory | spec-narrate-cli | FR-006 | UR-007]
#[test]
fn relative_input_prints_the_absolute_output_path() {
    let ready = Ready::new();
    let work = scratch();
    write_text(work.path(), "rel.txt", "Hello there.");
    let out = ready.nar().arg("rel.txt").dir(work.path()).run();
    out.assert_code(0);
    let wav = abs(&work.path().join("rel.wav"));
    assert!(wav.is_file());
    assert!(stdout_is_path(&out, &wav), "path rule: printed paths are std::path::absolute of the typed value\n{}", out.context());
}

// [Scenario: Override | spec-narrate-generation Req 2 | FR-006 | UR-007]
#[test]
fn dash_o_overrides_the_output_path() {
    let ready = Ready::new();
    let work = scratch();
    let script = write_text(work.path(), "a.txt", "Hello there.");
    fs::create_dir(work.path().join("out")).unwrap();
    let target = work.path().join("out").join("b.wav");
    let out = ready.nar().arg(&script).arg("-o").arg(&target).run();
    out.assert_code(0);
    assert!(target.is_file());
    assert!(!work.path().join("a.wav").exists(), "the default output must not be created");
    assert!(stdout_is_path(&out, &abs(&target)));

    // Long form.
    let target2 = work.path().join("out").join("c.wav");
    let out = ready.nar().arg(&script).arg("--output").arg(&target2).run();
    out.assert_code(0);
    assert!(target2.is_file());
}

// [Grammar: -o relative | spec-narrate-cli | FR-006 | UR-007]
#[test]
fn relative_dash_o_is_resolved_against_the_current_directory() {
    let ready = Ready::new();
    let work = scratch();
    write_text(work.path(), "a.txt", "Hello there.");
    fs::create_dir(work.path().join("sub")).unwrap();
    let out = ready.nar().args(["a.txt", "-o", "sub\\x.wav"]).dir(work.path()).run();
    out.assert_code(0);
    let wav = abs(&work.path().join("sub").join("x.wav"));
    assert!(wav.is_file());
    assert!(stdout_is_path(&out, &wav), "{}", out.context());
}

// [Scenario: Output directory missing | spec-narrate-generation Req 2 | FR-006 | NFR-007]
#[test]
fn missing_output_directory_exits_3_before_any_worker_starts() {
    let data = scratch(); // empty: precondition 3 comes before the readiness check
    let work = scratch();
    let script = write_text(work.path(), "a.txt", "Hello there.");
    let missing = work.path().join("nodir").join("a.wav");
    let out = Nar::new(data.path()).arg(&script).arg("-o").arg(&missing).run();
    out.assert_error(
        3,
        "output-dir-missing",
        &format!("output directory does not exist: {}", work.path().join("nodir").display()),
        "create the directory or choose another -o path",
    );
    out.assert_stderr_contains("nodir");
    assert!(!work.path().join("nodir").exists(), "the directory must not be created");

    // Relative form: <dir> is the parent exactly as typed.
    let out = Nar::new(data.path()).args(["a.txt", "-o", "nodir\\a.wav"]).dir(work.path()).run();
    out.assert_error(
        3,
        "output-dir-missing",
        "output directory does not exist: nodir",
        "create the directory or choose another -o path",
    );
}

// [Preconditions: output is not a directory | spec-narrate-generation | FR-006]
#[test]
fn output_path_that_is_a_directory_exits_3() {
    let data = scratch();
    let work = scratch();
    let script = write_text(work.path(), "a.txt", "Hello there.");
    let dir_as_output = work.path().join("outdir");
    fs::create_dir(&dir_as_output).unwrap();
    let typed = dir_as_output.display().to_string();
    let out = Nar::new(data.path()).arg(&script).arg("-o").arg(&dir_as_output).run();
    out.assert_error(
        3,
        "output-is-directory",
        &format!("output path is a directory: {typed}"),
        &format!("give a file name, for example -o {typed}\\out.wav"),
    );
}

// [Scenario: Output equals input | spec-narrate-generation Req 2 | FR-006 | NFR-007]
#[test]
fn output_that_resolves_to_the_input_is_rejected_and_the_input_is_untouched() {
    let data = scratch();
    let work = scratch();
    let script = write_text(work.path(), "intro.txt", "Original content.");
    let original = fs::read(&script).unwrap();
    let log = work.path().join("worker.log");
    fs::create_dir(work.path().join("sub")).unwrap();

    // Same file spelled with different case, and through `sub\..`.
    let spellings = [
        work.path().join("INTRO.TXT"),
        work.path().join("sub").join("..").join("intro.txt"),
        script.clone(),
    ];
    for typed in spellings {
        let out = Nar::new(data.path()).worker().log(&log).arg(&script).arg("-o").arg(&typed).run();
        out.assert_error(
            3,
            "output-is-input",
            &format!("the output would overwrite the input file: {}", typed.display()),
            "choose a different -o path",
        );
        assert_eq!(fs::read(&script).unwrap(), original, "input must be byte-identical");
    }
    assert!(!log.exists(), "no worker may start");
}

// [Scenario: Output equals input (FR-006 wording) | docs/requirements FR-006 | FR-006]
#[test]
fn output_equal_to_input_with_a_relative_spelling_is_rejected() {
    let data = scratch();
    let work = scratch();
    write_text(work.path(), "intro.txt", "Original content.");
    let out = Nar::new(data.path()).args(["intro.txt", "-o", "intro.txt"]).dir(work.path()).run();
    out.assert_error(
        3,
        "output-is-input",
        "the output would overwrite the input file: intro.txt",
        "choose a different -o path",
    );
    assert_eq!(fs::read_to_string(work.path().join("intro.txt")).unwrap(), "Original content.");
}

// ---------------------------------------------------------------------------------------
// Existing outputs
// ---------------------------------------------------------------------------------------

// [Scenario: Existing output overwritten | spec-narrate-generation Req 12 | FR-006]
#[test]
fn existing_output_is_replaced_and_no_temporary_file_remains() {
    let ready = Ready::new();
    let work = scratch();
    let script = write_text(work.path(), "intro.md", "# Intro\n\nHello there.");
    let wav = write_bytes(work.path(), "intro.wav", b"OLD CONTENT, NOT A WAV");
    let out = ready.nar().arg(&script).run();
    out.assert_code(0);
    let bytes = fs::read(&wav).unwrap();
    assert_ne!(bytes, b"OLD CONTENT, NOT A WAV".to_vec());
    assert!(wav_samples(&wav) > 0, "the file holds the new audio");
    assert!(tmp_leftovers(work.path()).is_empty(), "{:?}", list_dir(work.path()));
}

// [Scenario: Existing output survives failure | spec-narrate-generation Req 12 | FR-006]
#[test]
fn existing_output_is_byte_identical_after_a_failed_run() {
    let ready = Ready::new();
    let work = scratch();
    let script = write_text(work.path(), "intro.md", "# Intro\n\nHello there.");
    let wav = write_bytes(work.path(), "intro.wav", b"PRECIOUS EARLIER OUTPUT");
    let before = fs::read(&wav).unwrap();
    for (var, value) in [("NARRATE_TEST_FAIL_CHUNK", "1"), ("NARRATE_TEST_CRASH_CHUNK", "1"), ("NARRATE_TEST_WATERMARK_FAIL", "1")] {
        let out = ready.nar().env(var, value).arg(&script).run();
        out.assert_code(1);
        assert_eq!(fs::read(&wav).unwrap(), before, "{var}: earlier output must be unchanged");
        assert!(tmp_leftovers(work.path()).is_empty(), "{var}");
    }
}

// [Preconditions: existing output is not read-only | spec-narrate-generation | FR-006 | NFR-007]
#[test]
fn read_only_existing_output_exits_3_before_any_worker_starts() {
    let data = scratch();
    let work = scratch();
    let script = write_text(work.path(), "a.txt", "Hello there.");
    let wav = write_bytes(work.path(), "a.wav", b"READ ONLY CONTENT");
    let mut perms = fs::metadata(&wav).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&wav, perms).unwrap();
    let _unlock = Unlock(wav.clone());
    let log = work.path().join("worker.log");
    let out = Nar::new(data.path()).worker().log(&log).arg(&script).run();
    out.assert_error_prefix(
        3,
        "output-not-writable",
        &format!("output path is not writable: {} (", wav.display()),
        "choose another directory, or close programs that use the file",
    );
    assert_eq!(fs::read(&wav).unwrap(), b"READ ONLY CONTENT".to_vec());
    assert!(!log.exists());
}

// [Scenario: Permission denied | spec-narrate-generation Req 12 | FR-006 | NFR-007 | NFR-003]
#[test]
fn deny_write_acl_on_the_output_directory_exits_3_without_starting_a_worker() {
    let ready = Ready::new();
    let work = scratch();
    let script = write_text(work.path(), "a.txt", "Hello there.");
    let denied_dir = work.path().join("locked");
    fs::create_dir(&denied_dir).unwrap();
    let target = denied_dir.join("a.wav");
    let log = work.path().join("worker.log");
    let _acl = DenyWrite::apply(&denied_dir);
    let out = ready.nar().log(&log).arg(&script).arg("-o").arg(&target).arg("--verbose").run();
    out.assert_error_prefix(
        3,
        "output-not-writable",
        &format!("output path is not writable: {} (", target.display()),
        "choose another directory, or close programs that use the file",
    );
    assert!(
        !out.stderr.lines().any(|l| l.starts_with("verbose: worker pid")),
        "no worker may have been started\n{}",
        out.context()
    );
    assert!(!log.exists());
    assert!(!target.exists());
}

// ---------------------------------------------------------------------------------------
// WAV format and size limit
// ---------------------------------------------------------------------------------------

// [Scenario: Format + Fake worker drives a full run | spec-narrate-generation Req 11 / spec-narrate-worker Req 9 | FR-006]
#[test]
fn output_is_a_24k_mono_16_bit_wav_whose_sample_count_is_chunks_plus_pause() {
    let ready = Ready::new();
    let work = scratch();
    let script = write_text(work.path(), "one.txt", "Hello there.");
    let out = ready.nar().env("FAKE_WORKER_SAMPLES_PER_CHAR", "100").arg(&script).run();
    out.assert_code(0);
    let wav = work.path().join("one.wav");
    let samples = assert_output_wav_format(&wav);
    // "Hello there." has 12 characters: 12 x 100 returned samples plus the 12000-sample
    // paragraph pause that follows the last chunk.
    assert_eq!(samples, 12 * 100 + 12_000);
}

// [Scenario: Size limit | spec-narrate-generation Req 10 | NFR-005 | NFR-008]
#[test]
fn size_limit_hook_fails_with_wav_too_large_and_leaves_nothing() {
    let ready = Ready::new();
    let work = scratch();
    let script = write_text(work.path(), "big.txt", "Hello there.");
    let out = ready.nar().env("NARRATE_TEST_WAV_LIMIT_BYTES", "1000").arg(&script).run();
    out.assert_error(
        1,
        "wav-too-large",
        "the audio would exceed the WAV size limit of 1000 bytes",
        "split the script into smaller files",
    );
    out.assert_stderr_contains("1000");
    assert!(wav_or_tmp_files(work.path()).is_empty(), "no output or temporary file may exist");
}

// [RULE-004 | spec-narrate-generation | NFR-005]
#[test]
fn size_limit_boundary_exact_limit_succeeds_and_one_byte_less_fails() {
    let ready = Ready::new();
    let work = scratch();
    let script = write_text(work.path(), "edge.txt", "Hi.");
    // One chunk of exactly 100 samples plus the 12000-sample paragraph pause = 12100 samples.
    let exact = 44 + 2 * 12_100u64;
    let ok = ready
        .nar()
        .env("FAKE_WORKER_SYNTH_SAMPLES", "100")
        .env("NARRATE_TEST_WAV_LIMIT_BYTES", exact.to_string())
        .arg(&script)
        .run();
    ok.assert_code(0);
    let wav = work.path().join("edge.wav");
    assert_eq!(fs::metadata(&wav).unwrap().len(), exact, "file size equals the limit");
    fs::remove_file(&wav).unwrap();

    let less = exact - 1;
    let fail = ready
        .nar()
        .env("FAKE_WORKER_SYNTH_SAMPLES", "100")
        .env("NARRATE_TEST_WAV_LIMIT_BYTES", less.to_string())
        .arg(&script)
        .run();
    fail.assert_error(
        1,
        "wav-too-large",
        &format!("the audio would exceed the WAV size limit of {less} bytes"),
        "split the script into smaller files",
    );
    assert!(wav_or_tmp_files(work.path()).is_empty());
}

// [Scenario: Size limit + Existing output survives failure | spec-narrate-generation Req 10, 12 | NFR-005 | FR-006]
#[test]
fn size_limit_failure_keeps_an_earlier_output_unchanged() {
    let ready = Ready::new();
    let work = scratch();
    let script = write_text(work.path(), "big.txt", "Hello there.");
    let wav = write_bytes(work.path(), "big.wav", b"EARLIER");
    let out = ready.nar().env("NARRATE_TEST_WAV_LIMIT_BYTES", "1000").arg(&script).run();
    out.assert_code(1);
    assert_eq!(fs::read(&wav).unwrap(), b"EARLIER".to_vec());
    assert!(tmp_leftovers(work.path()).is_empty());
}

// [Default limit | spec-narrate-generation | NFR-005]
#[test]
fn default_size_limit_does_not_interfere_with_normal_runs() {
    let ready = Ready::new();
    let work = scratch();
    let script = write_text(work.path(), "ok.txt", "Hello there.");
    ready.nar().arg(&script).run().assert_code(0);
}
