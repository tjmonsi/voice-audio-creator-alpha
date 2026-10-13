//! CLI contract tests: usage errors, help, version, stream discipline, error message format,
//! verbose details, test-hook validation and the absence of any watermark option.
//!
//! Sources: specs/spec-narrate-cli.md (Requirements: Usage errors, Help and version, Stream
//! discipline, Error message format, Exit codes, Test hooks honored only in test builds) and
//! specs/spec-narrate-test-strategy.md (Missing test-kit binary is reported). No
//! implementation was read.

mod common;

use common::*;

fn snapshot(dir: &std::path::Path) -> Vec<String> {
    walk(dir)
        .into_iter()
        .map(|p| p.strip_prefix(dir).unwrap().display().to_string())
        .collect()
}

// ---------------------------------------------------------------------------------------
// Usage errors (exit 2)
// ---------------------------------------------------------------------------------------

// [Scenario: Unknown option | spec-narrate-cli | FR-024 | UR-006 | NFR-008]
#[test]
fn unknown_option_exits_2_with_the_option_and_usage_on_stderr() {
    let data = scratch();
    let out = Nar::new(data.path()).arg("--bogus").run();
    out.assert_code(2);
    out.assert_stderr_contains("--bogus");
    out.assert_stderr_contains("Usage:");
    assert_eq!(out.stdout, "", "usage errors go to stderr only");
    assert!(is_empty_dir(data.path()), "usage errors touch nothing");
}

// [Scenario: Invalid voice value | spec-narrate-cli | FR-007 | FR-024 | NFR-008]
#[test]
fn invalid_voice_value_exits_2_and_lists_the_valid_values() {
    let data = scratch();
    let cwd = scratch();
    let out = Nar::new(data.path()).args(["script.txt", "--voice", "robot"]).dir(cwd.path()).run();
    out.assert_code(2);
    for needle in ["robot", "female", "male", "builtin"] {
        out.assert_stderr_contains(needle);
    }
    assert!(is_empty_dir(data.path()));
    assert!(is_empty_dir(cwd.path()), "usage errors happen before touching the file system");
}

// [Scenario: Invalid voice value | spec-narrate-cli | FR-007 | FR-024]
#[test]
fn voice_values_are_case_sensitive_and_empty_is_rejected() {
    // Boundary / null-like inputs for --voice.
    for bad in ["Female", "MALE", "", "female ", "builtin2"] {
        let data = scratch();
        let out = Nar::new(data.path()).args(["script.txt", "--voice", bad]).run();
        out.assert_code(2);
        out.assert_stderr_contains("Usage:");
    }
}

// [Scenario: Invalid device value | spec-narrate-cli | FR-013 | FR-024 | NFR-008]
#[test]
fn invalid_device_value_exits_2_and_lists_the_valid_values() {
    let data = scratch();
    let cwd = scratch();
    let out = Nar::new(data.path()).args(["script.txt", "--device", "gpu"]).dir(cwd.path()).run();
    out.assert_code(2);
    for needle in ["auto", "cuda", "cpu"] {
        out.assert_stderr_contains(needle);
    }
    assert!(is_empty_dir(cwd.path()));
}

// [Scenario: Conflicting voice options | spec-narrate-cli | FR-009 | FR-024 | NFR-008]
#[test]
fn voice_and_voice_file_together_exit_2_and_create_nothing() {
    let data = scratch();
    let dir = scratch();
    write_text(dir.path(), "script.txt", "Hello.");
    write_bytes(dir.path(), "my.wav", &pcm16_clip(24_000, 168_000));
    let before = snapshot(dir.path());
    let out = Nar::new(data.path())
        .args(["script.txt", "--voice", "male", "--voice-file", "my.wav"])
        .dir(dir.path())
        .run();
    out.assert_code(2);
    out.assert_stderr_contains("cannot be used with");
    assert_eq!(snapshot(dir.path()), before, "no file may be created in the script directory");
    assert!(is_empty_dir(data.path()));
}

// [Scenario: No arguments | spec-narrate-cli | FR-024 | NFR-008]
#[test]
fn no_arguments_exit_2_with_usage() {
    let data = scratch();
    let out = Nar::new(data.path()).run();
    out.assert_code(2);
    out.assert_stderr_contains("Usage:");
}

// [Requirement: Usage errors (missing <FILE>) | spec-narrate-cli | FR-024 | NFR-008]
#[test]
fn options_without_a_file_exit_2() {
    let data = scratch();
    let out = Nar::new(data.path()).args(["--voice", "male"]).run();
    out.assert_code(2);
    out.assert_stderr_contains("Usage:");
}

// [Grammar: setup and status accept only --verbose and --help | spec-narrate-cli | FR-024]
#[test]
fn setup_and_status_reject_generation_options() {
    for args in [
        vec!["status", "--voice", "male"],
        vec!["status", "--device", "cpu"],
        vec!["status", "-o", "x.wav"],
        vec!["setup", "--voice-file", "x.wav"],
        vec!["setup", "-o", "x.wav"],
    ] {
        let data = scratch();
        let out = Nar::new(data.path()).args(&args).run();
        out.assert_code(2);
        out.assert_stderr_contains("Usage:");
        assert!(is_empty_dir(data.path()), "{args:?}");
    }
}

// [Grammar: subcommand names win only when exact | spec-narrate-cli | FR-001]
#[test]
fn a_script_named_setup_md_is_a_file_not_the_setup_command() {
    let data = scratch();
    let cwd = scratch();
    let out = Nar::new(data.path()).arg("setup.md").dir(cwd.path()).run();
    out.assert_error(3, "input-not-found", "input file not found: setup.md", "check the path and try again");
}

// [Grammar: --verbose position | spec-narrate-cli | FR-024]
#[test]
fn verbose_is_accepted_before_or_after_the_subcommand_and_file() {
    let before = {
        let d = scratch();
        Nar::new(d.path()).args(["--verbose", "status"]).run()
    };
    let after = {
        let d = scratch();
        Nar::new(d.path()).args(["status", "--verbose"]).run()
    };
    before.assert_code(4);
    after.assert_code(4);
    assert_eq!(before.stdout.lines().last(), Some("status: not ready (run narrate setup)"));
    assert_eq!(after.stdout.lines().last(), Some("status: not ready (run narrate setup)"));

    let cwd = scratch();
    for args in [vec!["--verbose", "nofile.txt"], vec!["nofile.txt", "--verbose"]] {
        let d = scratch();
        let out = Nar::new(d.path()).args(&args).dir(cwd.path()).run();
        assert!(
            out.has_stderr_line("error[input-not-found]: input file not found: nofile.txt"),
            "{args:?}\n{}",
            out.context()
        );
        out.assert_code(3);
    }
}

// ---------------------------------------------------------------------------------------
// Help and version
// ---------------------------------------------------------------------------------------

// [Scenario: Help lists commands and options | spec-narrate-cli | FR-024 | NFR-008]
#[test]
fn help_lists_commands_and_options_on_stdout() {
    for flag in ["--help", "-h"] {
        let data = scratch();
        let out = Nar::new(data.path()).arg(flag).run();
        out.assert_code(0);
        for needle in ["setup", "status", "--output", "--voice", "--voice-file", "--device", "--verbose", "--version"] {
            assert!(out.stdout.contains(needle), "{flag}: stdout must contain {needle}\n{}", out.context());
        }
        assert_eq!(out.stderr, "", "{flag}: help goes to stdout only");
        assert!(is_empty_dir(data.path()));
    }
}

// [Scenario: Version | spec-narrate-cli | FR-024 | NFR-008]
#[test]
fn version_line_in_a_test_build_names_the_hooks() {
    for flag in ["--version", "-V"] {
        let data = scratch();
        let out = Nar::new(data.path()).arg(flag).run();
        out.assert_code(0);
        assert!(out.stdout.starts_with("narrate 0.1.0"), "{}", out.context());
        // The automated suite always runs against a test-hooks build.
        assert_eq!(out.stdout.trim_end(), "narrate 0.1.0 (test hooks enabled)");
        assert_eq!(out.stderr, "");
    }
}

// [Scenario: No watermark option | spec-narrate-cli | FR-018 | UR-001]
#[test]
fn no_help_text_offers_a_watermark_option() {
    let mut texts = Vec::new();
    for args in [vec!["--help"], vec!["setup", "--help"], vec!["status", "--help"]] {
        let data = scratch();
        let out = Nar::new(data.path()).args(&args).run();
        out.assert_code(0);
        texts.push((args.join(" "), out.stdout.clone()));
    }
    for (what, text) in texts {
        assert!(!text.trim().is_empty(), "{what} printed help");
        for line in text.lines() {
            // Regular expression `--\S*watermark`, evaluated per whitespace-delimited token.
            for token in line.split_whitespace() {
                let mut from = 0;
                while let Some(pos) = token[from..].find("--") {
                    let rest = &token[from + pos + 2..];
                    assert!(
                        !rest.contains("watermark"),
                        "{what}: line mentions a watermark option: {line}"
                    );
                    from += pos + 2;
                }
            }
        }
    }
}

// [Scenario: No watermark option | spec-narrate-cli | FR-018]
#[test]
fn watermark_flags_are_rejected_as_unknown_options() {
    for flag in ["--no-watermark", "--watermark", "--disable-watermark"] {
        let data = scratch();
        let out = Nar::new(data.path()).args(["script.txt", flag]).run();
        out.assert_code(2);
        out.assert_stderr_contains(flag);
        out.assert_stderr_contains("Usage:");
    }
}

// ---------------------------------------------------------------------------------------
// Stream discipline and error message format
// ---------------------------------------------------------------------------------------

// [Scenario: Error goes to stderr + Cause and next action | spec-narrate-cli | UR-006 | NFR-007 | NFR-008]
#[test]
fn missing_input_prints_cause_and_fix_on_stderr_and_nothing_on_stdout() {
    let data = scratch();
    let cwd = scratch();
    let out = Nar::new(data.path()).arg("nofile.txt").dir(cwd.path()).run();
    assert_eq!(out.stdout, "", "errors never go to stdout");
    out.assert_error(
        3,
        "input-not-found",
        "input file not found: nofile.txt",
        "check the path and try again",
    );
    let cause = out.find_stderr_prefix("error[input-not-found]: ").unwrap();
    assert!(cause.contains("nofile.txt"));
    // Without --verbose no detail line appears.
    out.assert_stderr_lacks("detail:");
}

// [Requirement: Error message format | spec-narrate-cli | NFR-007]
#[test]
fn error_line_is_followed_immediately_by_the_fix_line_for_several_codes() {
    let cwd = scratch();
    write_bytes(cwd.path(), "story.docx", b"x");
    std::fs::create_dir(cwd.path().join("folder.txt")).unwrap();
    // (args, exit, code, cause, fix)
    let cases: Vec<(Vec<&str>, i32, &str, &str, &str)> = vec![
        (
            vec!["story.docx"],
            3,
            "input-extension",
            "unsupported script type: story.docx",
            "use a .md, .markdown, or .txt file",
        ),
        (
            vec!["folder.txt"],
            3,
            "input-not-a-file",
            "input path is not a file: folder.txt",
            "pass a script file, not a directory",
        ),
        (
            vec!["nofile.md"],
            3,
            "input-not-found",
            "input file not found: nofile.md",
            "check the path and try again",
        ),
    ];
    for (args, exit, code, cause, fix) in cases {
        let data = scratch();
        let out = Nar::new(data.path()).args(&args).dir(cwd.path()).run();
        out.assert_error(exit, code, cause, fix);
    }
}

fn failing_generation(ready: &Ready, scratch_dir: &std::path::Path, verbose: bool) -> Out {
    let script = write_text(scratch_dir, "story.txt", "Hello there.");
    let traceback = "Traceback (most recent call last):\n  File \"fake.py\", line 7, in synth\nRuntimeError: model exploded";
    let mut nar = ready
        .nar()
        .env("NARRATE_TEST_FAIL_CHUNK", "1")
        .env("FAKE_WORKER_TRACEBACK", traceback)
        .arg(&script);
    if verbose {
        nar = nar.arg("--verbose");
    }
    nar.run()
}

// [Scenario: Verbose error details | spec-narrate-cli | FR-024 | NFR-007]
#[test]
fn verbose_adds_detail_lines_carrying_the_worker_traceback() {
    let ready = Ready::new();
    let work = scratch();
    let out = failing_generation(&ready, work.path(), true);
    out.assert_code(1);
    assert!(out.stderr.contains("error[chunk-failed]"), "{}", out.context());
    assert!(out.stderr.lines().any(|l| l.starts_with("  detail: ")), "{}", out.context());
    for tb_line in [
        "Traceback (most recent call last):",
        "File \"fake.py\", line 7, in synth",
        "RuntimeError: model exploded",
    ] {
        assert!(
            out.stderr.lines().any(|l| l.starts_with("  detail: ") && l.contains(tb_line)),
            "detail lines must carry {tb_line:?}\n{}",
            out.context()
        );
    }
    // Detail lines come after the fix line of the error they belong to.
    let fix_at = out.stderr.lines().position(|l| l.starts_with("  fix: ")).unwrap();
    let first_detail = out.stderr.lines().position(|l| l.starts_with("  detail: ")).unwrap();
    assert!(first_detail > fix_at);
}

// [Scenario: Details hidden without verbose | spec-narrate-cli | FR-024 | NFR-007]
#[test]
fn without_verbose_no_detail_lines_and_no_traceback_appear() {
    let ready = Ready::new();
    let work = scratch();
    let out = failing_generation(&ready, work.path(), false);
    out.assert_code(1);
    assert!(out.stderr.contains("error[chunk-failed]"), "{}", out.context());
    out.assert_stderr_lacks("detail:");
    out.assert_stderr_lacks("Traceback (most recent call last)");
    out.assert_stderr_lacks("model exploded");
    assert!(out.stderr.lines().any(|l| l.starts_with("  fix: ")));
}

// ---------------------------------------------------------------------------------------
// Test hooks
// ---------------------------------------------------------------------------------------

// [Scenario: Invalid hook value | spec-narrate-cli | FR-005 | NFR-008]
#[test]
fn invalid_hook_values_exit_2_naming_the_variable() {
    let cwd = scratch();
    // Invalid, boundary-below and non-numeric values for each numeric hook.
    let cases = [
        ("NARRATE_TEST_FAIL_CHUNK", "abc"),
        ("NARRATE_TEST_FAIL_CHUNK", "0"),
        ("NARRATE_TEST_FAIL_CHUNK", "-1"),
        ("NARRATE_TEST_CRASH_CHUNK", "xyz"),
        ("NARRATE_TEST_CRASH_CHUNK", "0"),
        ("NARRATE_TEST_WAV_LIMIT_BYTES", "abc"),
        ("NARRATE_TEST_WAV_LIMIT_BYTES", "43"),
    ];
    for (var, value) in cases {
        let data = scratch();
        let out = Nar::new(data.path()).arg("nofile.txt").env(var, value).dir(cwd.path()).run();
        out.assert_error(
            2,
            "invalid-test-hook",
            &format!("{var} has an invalid value: {value}"),
            "unset it or give a valid value",
        );
    }
}

// [Scenario: Invalid hook value | spec-narrate-cli | FR-005 | NFR-005]
#[test]
fn smallest_valid_hook_values_are_accepted() {
    let cwd = scratch();
    for (var, value) in [
        ("NARRATE_TEST_FAIL_CHUNK", "1"),
        ("NARRATE_TEST_CRASH_CHUNK", "1"),
        ("NARRATE_TEST_WAV_LIMIT_BYTES", "44"),
        ("NARRATE_TEST_WATERMARK_FAIL", "1"),
    ] {
        let data = scratch();
        let out = Nar::new(data.path()).arg("nofile.txt").env(var, value).dir(cwd.path()).run();
        // Past hook parsing the run reaches input validation and fails there (exit 3), not 2.
        out.assert_error(3, "input-not-found", "input file not found: nofile.txt", "check the path and try again");
    }
}

// ---------------------------------------------------------------------------------------
// Test-kit helper contract
// ---------------------------------------------------------------------------------------

// [Scenario: Missing test-kit binary is reported | spec-narrate-test-strategy | FR-005]
#[test]
fn missing_test_kit_binary_panics_naming_the_workspace_test_command() {
    let result = std::panic::catch_unwind(|| kit_bin("narrate-test-does-not-exist"));
    let payload = result.expect_err("a missing binary must panic, not be skipped");
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|m| m.to_string()))
        .unwrap_or_default();
    assert!(
        message.contains("cargo test --workspace --all-features"),
        "panic message must name the command: {message}"
    );
}

// [Requirement: Test kit | spec-narrate-test-strategy | FR-005]
#[test]
fn all_test_kit_binaries_are_built_next_to_narrate() {
    for name in ["narrate-test-worker", "narrate-test-uv", "narrate-test-proxy", "narrate-test-signal"] {
        assert!(kit_bin(name).is_file(), "{name}");
    }
}

// [Helper self-check | spec-narrate-test-strategy | NFR-001]
#[test]
fn sha256_helper_matches_published_vectors() {
    assert_sha256_helper_works();
}
