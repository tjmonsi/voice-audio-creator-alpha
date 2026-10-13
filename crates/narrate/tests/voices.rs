//! Voice lines, voice requests, built-in voice readiness and custom clip rules
//! (FR-007, FR-008, FR-009, FR-003 warnings).
//!
//! Sources: specs/spec-narrate-generation.md (Requirements 3, 4, 6, 9), specs/spec-narrate-cli.md
//! (line catalog, error catalog), docs/requirements.md FR-007 to FR-009. The fake worker logs
//! every request, including the size of the clip a `voice` request names at that moment. No
//! implementation was read.

mod common;

use common::*;
use serde_json::Value;
use std::path::Path;

const FRAMES_5S: u64 = 120_000;
const FRAMES_10S: u64 = 240_000;

fn voice_entry(log: &Path) -> Value {
    let v = requests_of(log, "voice");
    assert_eq!(v.len(), 1, "exactly one voice request is expected: {v:?}");
    v.into_iter().next().unwrap()
}

fn voice_lines(out: &Out) -> Vec<&str> {
    out.stderr.lines().filter(|l| l.starts_with("voice:")).collect()
}

fn script(work: &Path) -> std::path::PathBuf {
    write_text(work, "script.txt", "Hello there. This is a test.")
}

// ---------------------------------------------------------------------------------------
// FR-007 / FR-008: presets
// ---------------------------------------------------------------------------------------

// [Scenario: Default is female | spec-narrate-generation Req 6 | FR-007 | UR-002]
#[test]
fn default_voice_is_female_with_the_data_directory_clip() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let s = script(work.path());
    let out = ready.nar().log(&log).arg(&s).run();
    out.assert_code(0);
    let expected = format!("voice: female ({})", ready.d().join("voices").join("female.wav").display());
    assert_eq!(voice_lines(&out), vec![expected.as_str()], "{}", out.context());

    let v = voice_entry(&log);
    assert_eq!(v["request"]["voice_id"], "female");
    assert!(same_path(v["request"]["clip"].as_str().unwrap(), &ready.d().join("voices").join("female.wav")));
}

// [Scenario: Male and builtin | spec-narrate-generation Req 6 | FR-007 | FR-008 | UR-002]
#[test]
fn male_and_builtin_voice_lines_are_exact() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());

    let out = ready.nar().arg(&s).args(["--voice", "male"]).run();
    out.assert_code(0);
    let male = format!("voice: male ({})", ready.d().join("voices").join("male.wav").display());
    assert_eq!(voice_lines(&out), vec![male.as_str()]);

    let out = ready.nar().arg(&s).args(["--voice", "builtin"]).run();
    out.assert_code(0);
    assert_eq!(voice_lines(&out), vec!["voice: builtin"]);
    assert!(out.has_stderr_line("voice: builtin"));
}

// [Scenario: Default is female | spec-narrate-generation Req 6 | FR-007]
#[test]
fn explicit_female_equals_the_default() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());
    let out = ready.nar().arg(&s).args(["--voice", "female"]).run();
    out.assert_code(0);
    let expected = format!("voice: female ({})", ready.d().join("voices").join("female.wav").display());
    assert_eq!(voice_lines(&out), vec![expected.as_str()]);
}

// [Scenario: Voice request content | spec-narrate-generation Req 9 | FR-007 | FR-008]
#[test]
fn voice_requests_carry_voice_id_and_clip_path() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());

    let log = work.path().join("male.log");
    ready.nar().log(&log).arg(&s).args(["--voice", "male"]).run().assert_code(0);
    let v = voice_entry(&log);
    assert_eq!(v["request"]["voice_id"], "male");
    assert!(same_path(v["request"]["clip"].as_str().unwrap(), &ready.d().join("voices").join("male.wav")));
    assert!(requests_of(&log, "synth").iter().all(|e| e["request"]["voice_id"] == "male"));

    let log = work.path().join("builtin.log");
    ready.nar().log(&log).arg(&s).args(["--voice", "builtin"]).run().assert_code(0);
    let v = voice_entry(&log);
    assert_eq!(v["request"]["voice_id"], "builtin");
    assert!(v["request"]["clip"].is_null(), "builtin sends clip null: {v}");
    assert!(requests_of(&log, "synth").iter().all(|e| e["request"]["voice_id"] == "builtin"));
}

// [Requirement 9: one voice request before the first synth | spec-narrate-generation | FR-007]
#[test]
fn voice_request_precedes_every_synth_request() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let s = write_text(work.path(), "multi.txt", "One.\n\nTwo.\n\nThree.");
    ready.nar().log(&log).arg(&s).run().assert_code(0);
    let ops = op_sequence(&log);
    assert_eq!(ops.iter().filter(|o| *o == "voice").count(), 1);
    let voice_at = ops.iter().position(|o| o == "voice").unwrap();
    let first_synth = ops.iter().position(|o| o == "synth").unwrap();
    assert!(voice_at < first_synth, "{ops:?}");
}

// [Scenario: Built-in conditionals missing | spec-narrate-generation Req 4 | FR-008 | NFR-007]
#[test]
fn builtin_voice_without_conds_pt_exits_4_and_advises_setup() {
    let ready = Ready::new();
    ready.remove("models/chatterbox-turbo/conds.pt");
    let work = scratch();
    let log = work.path().join("worker.log");
    let s = script(work.path());
    let out = ready.nar().log(&log).arg(&s).args(["--voice", "builtin"]).run();
    out.assert_error(
        4,
        "not-set-up",
        "setup is incomplete: turbo weights missing (conds.pt)",
        "run narrate setup",
    );
    out.assert_stderr_contains("conds.pt");
    out.assert_stderr_contains("narrate setup");
    assert!(!log.exists(), "no worker may start");
    assert!(wav_or_tmp_files(work.path()).is_empty());
}

// [Scenario: Built-in conditionals missing | spec-narrate-generation Req 4 | FR-008]
#[test]
fn missing_conds_pt_is_reported_for_the_default_voice_too() {
    let ready = Ready::new();
    ready.remove("models/chatterbox-turbo/conds.pt");
    let work = scratch();
    let s = script(work.path());
    let out = ready.nar().arg(&s).run();
    out.assert_code(4);
    out.assert_stderr_contains("conds.pt");
}

// [Scenario: Voice preparation fails | spec-narrate-generation Req 9 | FR-007 | NFR-007]
#[test]
fn voice_failed_maps_to_voice_prepare() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());
    let out = ready.nar().env("FAKE_WORKER_FAIL_OP", "voice:voice_failed").arg(&s).run();
    out.assert_error_prefix(1, "voice-prepare", "could not prepare the female voice: ", "use another clip or run narrate setup");
    assert!(wav_or_tmp_files(work.path()).is_empty());
}

// [Requirement 9: builtin_unavailable maps to voice-prepare | spec-narrate-generation | FR-008]
#[test]
fn builtin_unavailable_maps_to_voice_prepare() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());
    let out = ready
        .nar()
        .env("FAKE_WORKER_FAIL_OP", "voice:builtin_unavailable")
        .arg(&s)
        .args(["--voice", "builtin"])
        .run();
    out.assert_error_prefix(1, "voice-prepare", "could not prepare the builtin voice: ", "use another clip or run narrate setup");
}

// ---------------------------------------------------------------------------------------
// FR-003: unknown tag warnings (Requirement 6)
// ---------------------------------------------------------------------------------------

// [Scenario: Unknown tag warning | spec-narrate-generation Req 6 | FR-003]
#[test]
fn unknown_tag_warns_once_and_reaches_the_worker_unchanged() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let s = write_text(work.path(), "tags.txt", "Hi [dance] there [dance] [laugh].");
    let out = ready.nar().log(&log).arg(&s).run();
    out.assert_code(0);
    assert_eq!(out.count_stderr_line("warning: unknown tag [dance] will be read as text"), 1, "{}", out.context());
    assert!(!out.stderr.contains("unknown tag [laugh]"));
    let text = synth_texts(&log).join(" ");
    assert!(text.contains("[dance]") && text.contains("[laugh]"), "{text}");
}

// [Requirement 6: one warning per distinct unknown tag in chunk order | spec-narrate-generation | FR-003]
#[test]
fn distinct_unknown_tags_warn_in_order_after_the_voice_line() {
    let ready = Ready::new();
    let work = scratch();
    let s = write_text(work.path(), "tags.txt", "First [zeta] and [beta].\n\nSecond [alpha] and [beta] again.");
    let out = ready.nar().arg(&s).run();
    out.assert_code(0);
    let warnings: Vec<&str> = out.stderr.lines().filter(|l| l.starts_with("warning: unknown tag")).collect();
    assert_eq!(
        warnings,
        vec![
            "warning: unknown tag [zeta] will be read as text",
            "warning: unknown tag [beta] will be read as text",
            "warning: unknown tag [alpha] will be read as text",
        ]
    );
    let voice_at = out.stderr.lines().position(|l| l.starts_with("voice:")).unwrap();
    let first_warning = out.stderr.lines().position(|l| l.starts_with("warning: unknown tag")).unwrap();
    assert!(voice_at < first_warning);
}

// [Requirement 6: no warning for known or malformed tags | spec-narrate-generation | FR-003]
#[test]
fn known_and_unclosed_tags_produce_no_warning() {
    let ready = Ready::new();
    let work = scratch();
    let s = write_text(work.path(), "ok-tags.txt", "Well [chuckle] that [clear throat] worked [laugh and [sigh] done.");
    let out = ready.nar().arg(&s).run();
    out.assert_code(0);
    out.assert_stderr_lacks("warning: unknown tag");
}

// ---------------------------------------------------------------------------------------
// FR-009: custom clip
// ---------------------------------------------------------------------------------------

// [Scenario: Valid long clip | spec-narrate-generation Req 3 | FR-009 | UR-002]
#[test]
fn clip_longer_than_ten_seconds_prints_the_notice_and_sends_a_ten_second_prefix() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let s = script(work.path());
    let clip = write_bytes(work.path(), "my.wav", &pcm16_clip(24_000, 288_000)); // 12.0 s
    let out = ready.nar().log(&log).arg(&s).arg("--voice-file").arg(&clip).run();
    out.assert_code(0);

    let a = abs(&clip).display().to_string();
    let voice_line = format!("voice: file ({a})");
    assert!(out.has_stderr_line(&voice_line), "{}", out.context());
    assert!(voice_line.contains("my.wav"));
    let notice = format!("notice: {a} is 12.0 s long; only the first 10 s are used");
    assert!(out.has_stderr_line(&notice), "{}", out.context());
    assert!(out.stderr_line_index(&voice_line).unwrap() < out.stderr_line_index(&notice).unwrap());

    let v = voice_entry(&log);
    assert_eq!(v["request"]["voice_id"], "file");
    let sent = v["request"]["clip"].as_str().unwrap().to_string();
    let staging = ready.d().join("staging");
    assert!(
        sent.to_ascii_lowercase().starts_with(&staging.display().to_string().to_ascii_lowercase()),
        "the trimmed clip lives under <data>\\staging\\: {sent}"
    );
    assert_ne!(sent.to_ascii_lowercase(), a.to_ascii_lowercase(), "the original clip is not sent");
    assert_eq!(v["clip_exists"], true, "the clip must exist while the worker prepares the voice");
    assert_eq!(v["clip_frames"].as_u64(), Some(FRAMES_10S), "exactly 10.0 s at 24 kHz");
    assert!(!Path::new(&sent).exists(), "the trimmed clip must not exist after the command exits");
    let leftovers: Vec<String> = list_dir(&ready.d().join("staging")).into_iter().filter(|n| n.starts_with("voice-")).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

// [Scenario: Valid short clip | spec-narrate-generation Req 3 | FR-009]
#[test]
fn clip_between_five_and_ten_seconds_prints_no_notice_and_is_sent_as_is() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let s = script(work.path());
    let clip = write_bytes(work.path(), "my.wav", &pcm16_clip(24_000, 168_000)); // 7.0 s
    let out = ready.nar().log(&log).arg(&s).arg("--voice-file").arg(&clip).run();
    out.assert_code(0);
    out.assert_stderr_lacks("notice:");
    assert!(out.has_stderr_line(&format!("voice: file ({})", abs(&clip).display())));
    let v = voice_entry(&log);
    assert!(same_path(v["request"]["clip"].as_str().unwrap(), &clip), "{v}");
}

// [Scenario: Valid long clip, boundary at 10 s | spec-narrate-generation Req 3 | FR-009]
#[test]
fn exactly_ten_seconds_needs_no_trim_and_one_frame_more_does() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());

    let log = work.path().join("ten.log");
    let exact = write_bytes(work.path(), "ten.wav", &pcm16_clip(24_000, FRAMES_10S));
    let out = ready.nar().log(&log).arg(&s).arg("--voice-file").arg(&exact).run();
    out.assert_code(0);
    out.assert_stderr_lacks("notice:");
    assert!(same_path(voice_entry(&log)["request"]["clip"].as_str().unwrap(), &exact));

    let log = work.path().join("over.log");
    let over = write_bytes(work.path(), "over.wav", &pcm16_clip(24_000, FRAMES_10S + 1));
    let out = ready.nar().log(&log).arg(&s).arg("--voice-file").arg(&over).run();
    out.assert_code(0);
    let notice = format!("notice: {} is 10.0 s long; only the first 10 s are used", abs(&over).display());
    assert!(out.has_stderr_line(&notice), "{}", out.context());
    assert_eq!(voice_entry(&log)["clip_frames"].as_u64(), Some(FRAMES_10S));
}

// [Scenario: Exactly 5 s rejected + Short clip | spec-narrate-generation Req 3 | FR-009 | NFR-007]
#[test]
fn clip_of_five_seconds_or_less_is_rejected_and_one_frame_more_is_accepted() {
    let data = scratch(); // empty: clip validation comes before the readiness check
    let work = scratch();
    for (name, frames, shown) in [("short.wav", FRAMES_5S, "5.0"), ("four.wav", 96_000u64, "4.0")] {
        write_bytes(work.path(), name, &pcm16_clip(24_000, frames));
        write_text(work.path(), "a.txt", "Hello there.");
        let out = Nar::new(data.path()).args(["a.txt", "--voice-file", name]).dir(work.path()).run();
        out.assert_error(
            3,
            "voice-file-too-short",
            &format!("{name} is {shown} s long; the voice clip must be longer than 5 s"),
            "record or export a clip longer than 5 s (10 s or more is best)",
        );
        out.assert_stderr_contains("longer than 5 s");
    }

    let ready = Ready::new();
    let just_over = write_bytes(work.path(), "over5.wav", &pcm16_clip(24_000, FRAMES_5S + 1));
    let s = script(work.path());
    ready.nar().arg(&s).arg("--voice-file").arg(&just_over).run().assert_code(0);
}

// [Scenario: Not a WAV | spec-narrate-generation Req 3 | FR-009 | NFR-007]
#[test]
fn text_file_and_truncated_wav_are_unreadable() {
    let data = scratch();
    let work = scratch();
    write_text(work.path(), "a.txt", "Hello there.");
    write_text(work.path(), "fake.wav", "this is a text file renamed to .wav");
    write_bytes(work.path(), "cut.wav", &truncated_clip(24_000, 480_000, 2_000));
    for name in ["fake.wav", "cut.wav"] {
        let out = Nar::new(data.path()).args(["a.txt", "--voice-file", name]).dir(work.path()).run();
        out.assert_error_prefix(
            3,
            "voice-file-unreadable",
            &format!("{name} is not a readable WAV file ("),
            "export the clip as a WAV file and try again",
        );
        out.assert_stderr_contains(name);
    }
}

// [Scenario: Unsupported encoding | spec-narrate-generation Req 3 | FR-009 | NFR-007]
#[test]
fn a_law_clip_is_reported_as_unsupported_with_the_encoding_list() {
    let data = scratch();
    let work = scratch();
    write_text(work.path(), "a.txt", "Hello there.");
    write_bytes(work.path(), "odd.wav", &wav_bytes(6, 1, 8_000, 8, 80_000));
    let out = Nar::new(data.path()).args(["a.txt", "--voice-file", "odd.wav"]).dir(work.path()).run();
    out.assert_error(
        3,
        "voice-file-unsupported",
        "odd.wav uses an unsupported WAV encoding (format tag 6, 8 bits); supported encodings: PCM 8, 16, 24, or 32-bit integer, or 32-bit float",
        "convert the clip to 16-bit PCM WAV",
    );
    out.assert_stderr_contains("format tag 6, 8 bits");
    out.assert_stderr_contains("PCM 8, 16, 24, or 32-bit integer, or 32-bit float");
}

// [Scenario: Clip missing | spec-narrate-generation Req 3 | FR-009 | NFR-007]
#[test]
fn missing_clip_exits_3_naming_the_path_as_typed() {
    let data = scratch();
    let work = scratch();
    write_text(work.path(), "a.txt", "Hello there.");
    let out = Nar::new(data.path()).args(["a.txt", "--voice-file", "gone.wav"]).dir(work.path()).run();
    out.assert_error(3, "voice-file-not-found", "voice file not found: gone.wav", "check the path to the WAV clip");
    assert!(wav_or_tmp_files(work.path()).is_empty());
}

// [Requirement 3: any supported encoding, channel count and rate | spec-narrate-generation | FR-009]
#[test]
fn supported_encodings_channel_layouts_and_rates_are_accepted() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());
    // (name, tag, channels, rate, bits, frames) - each clip is 7.0 s
    let cases: [(&str, u16, u16, u32, u16, u64); 4] = [
        ("pcm8.wav", 1, 1, 8_000, 8, 56_000),
        ("pcm24-stereo.wav", 1, 2, 44_100, 24, 308_700),
        ("pcm32.wav", 1, 1, 16_000, 32, 112_000),
        ("float32.wav", 3, 1, 48_000, 32, 336_000),
    ];
    for (name, tag, ch, rate, bits, frames) in cases {
        let clip = write_bytes(work.path(), name, &wav_bytes(tag, ch, rate, bits, frames));
        let out = ready.nar().arg(&s).arg("--voice-file").arg(&clip).run();
        assert_eq!(out.code, 0, "{name}\n{}", out.context());
        out.assert_stderr_lacks("notice:");
    }
}

// [Requirement 6: trimmed clip keeps the source format | spec-narrate-generation | FR-009]
#[test]
fn trimming_keeps_the_sample_rate_of_a_float_clip() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let s = script(work.path());
    let clip = write_bytes(work.path(), "long-float.wav", &wav_bytes(3, 1, 48_000, 32, 576_000)); // 12.0 s
    let out = ready.nar().log(&log).arg(&s).arg("--voice-file").arg(&clip).run();
    out.assert_code(0);
    assert!(out.stderr.contains("only the first 10 s are used"));
    let v = voice_entry(&log);
    assert_eq!(v["clip_frames"].as_u64(), Some(480_000), "first 10 s at 48 kHz");
    assert_eq!(v["clip_sample_rate"].as_u64(), Some(48_000));
}

// [Path rule | spec-narrate-cli | FR-009]
#[test]
fn relative_voice_file_is_printed_and_sent_as_an_absolute_path() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    write_text(work.path(), "a.txt", "Hello there.");
    write_bytes(work.path(), "my.wav", &pcm16_clip(24_000, 168_000));
    let out = ready.nar().log(&log).args(["a.txt", "--voice-file", "my.wav"]).dir(work.path()).run();
    out.assert_code(0);
    let expected = abs(&work.path().join("my.wav"));
    assert!(out.has_stderr_line(&format!("voice: file ({})", expected.display())), "{}", out.context());
    assert!(same_path(voice_entry(&log)["request"]["clip"].as_str().unwrap(), &expected));
}

// [Scenario: Valid long clip | spec-narrate-generation Req 3 | FR-009 | FR-005]
#[test]
fn trimmed_clip_is_removed_even_when_the_run_fails() {
    let ready = Ready::new();
    let work = scratch();
    let s = script(work.path());
    let clip = write_bytes(work.path(), "my.wav", &pcm16_clip(24_000, 288_000));
    let out = ready
        .nar()
        .env("NARRATE_TEST_FAIL_CHUNK", "1")
        .arg(&s)
        .arg("--voice-file")
        .arg(&clip)
        .run();
    out.assert_code(1);
    let leftovers: Vec<String> = list_dir(&ready.d().join("staging")).into_iter().filter(|n| n.starts_with("voice-")).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}
