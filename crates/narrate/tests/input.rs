//! Input checks and the text sent to the worker (FR-001 to FR-004), plus the generation-side
//! readiness scenarios of FR-015.
//!
//! Sources: specs/spec-narrate-generation.md (Requirements 1, 4, 5), specs/spec-narrate-cli.md
//! (error catalog), specs/spec-narrate-text-pipeline.md (worked examples), docs/requirements.md.
//! A fabricated ready data directory, the fake worker and its request log are used; no
//! implementation was read.

mod common;

use common::*;
use std::path::Path;

fn listing(dir: &Path) -> Vec<String> {
    list_dir(dir)
}

fn all_synth_text(log: &Path) -> String {
    synth_texts(log).join("\n")
}

// ---------------------------------------------------------------------------------------
// Extension, existence, directory, readability
// ---------------------------------------------------------------------------------------

// [Scenario: Unsupported extension | spec-narrate-generation Req 1 | FR-001 | NFR-007 | NFR-008]
#[test]
fn unsupported_extension_exits_3_names_the_types_and_creates_nothing() {
    let data = scratch(); // empty: the extension check precedes the readiness check
    let work = scratch();
    write_bytes(work.path(), "story.docx", b"hello");
    let before = listing(work.path());
    let out = Nar::new(data.path()).arg("story.docx").dir(work.path()).run();
    out.assert_error(
        3,
        "input-extension",
        "unsupported script type: story.docx",
        "use a .md, .markdown, or .txt file",
    );
    for ext in [".md", ".markdown", ".txt"] {
        out.assert_stderr_contains(ext);
    }
    assert_eq!(listing(work.path()), before, "the directory must contain no new file");
    assert_eq!(out.stdout, "");
    assert!(is_empty_dir(data.path()));
}

// [Scenario: Unsupported extension | spec-narrate-generation Req 1 | FR-001]
#[test]
fn extension_is_checked_before_existence_and_lookalikes_are_rejected() {
    let work = scratch();
    for name in ["nofile.docx", "script", "script.md.bak", "script.mdx", "script.text"] {
        let data = scratch();
        let out = Nar::new(data.path()).arg(name).dir(work.path()).run();
        out.assert_error(
            3,
            "input-extension",
            &format!("unsupported script type: {name}"),
            "use a .md, .markdown, or .txt file",
        );
    }
}

// [Scenario: Missing file | spec-narrate-generation Req 1 | FR-001 | NFR-007]
#[test]
fn missing_input_file_exits_3_and_names_the_path_as_typed() {
    let data = scratch();
    let work = scratch();
    let out = Nar::new(data.path()).arg("nofile.txt").dir(work.path()).run();
    out.assert_error(3, "input-not-found", "input file not found: nofile.txt", "check the path and try again");
    assert_eq!(out.stdout, "");
    assert!(is_empty_dir(work.path()));
}

// [Preconditions: input is a file | spec-narrate-generation | FR-001]
#[test]
fn directory_as_input_exits_3_with_input_not_a_file() {
    let data = scratch();
    let work = scratch();
    std::fs::create_dir(work.path().join("scripts.txt")).unwrap();
    let out = Nar::new(data.path()).arg("scripts.txt").dir(work.path()).run();
    out.assert_error(
        3,
        "input-not-a-file",
        "input path is not a file: scripts.txt",
        "pass a script file, not a directory",
    );
}

// [Error catalog: input-unreadable | spec-narrate-cli | FR-001 | NFR-007]
#[test]
fn unreadable_input_exits_3_with_input_unreadable() {
    use std::os::windows::fs::OpenOptionsExt;
    let ready = Ready::new();
    let work = scratch();
    let p = write_text(work.path(), "story.txt", "Hello there.");
    let _exclusive = std::fs::OpenOptions::new().read(true).share_mode(0).open(&p).unwrap();
    let log = work.path().join("worker.log");
    let out = ready.nar().log(&log).arg("story.txt").dir(work.path()).run();
    out.assert_error_prefix(3, "input-unreadable", "cannot read story.txt: ", "check the file permissions");
    assert!(!log.exists(), "no worker may start");
}

// ---------------------------------------------------------------------------------------
// Encoding and empty scripts (run with a ready directory: the script is read after readiness)
// ---------------------------------------------------------------------------------------

// [Scenario: Invalid UTF-8 | spec-narrate-generation Req 5 | FR-001 | NFR-007]
#[test]
fn invalid_utf8_exits_3_naming_the_file_encoding_and_offset() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    // (file name, bytes, expected offset)
    let cases: [(&str, &[u8], usize); 3] = [
        ("bad.txt", &[0xFF], 0),
        ("bad2.txt", b"ab\xFFc", 2),
        ("bad3.md", b"\xEF\xBB\xBFa\xFF", 4),
    ];
    for (name, bytes, offset) in cases {
        write_bytes(work.path(), name, bytes);
        let out = ready.nar().log(&log).arg(name).dir(work.path()).run();
        out.assert_error(
            3,
            "input-encoding",
            &format!("{name} is not valid UTF-8 (invalid byte at offset {offset})"),
            "save the file with UTF-8 encoding",
        );
        out.assert_stderr_contains("UTF-8");
    }
    assert!(!log.exists(), "no worker may start");
    assert!(wav_or_tmp_files(work.path()).is_empty());
}

// [Scenario: Empty and whitespace-only | spec-narrate-generation Req 5 | FR-004 | NFR-007]
#[test]
fn empty_whitespace_and_code_only_scripts_exit_3_without_output() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    // Null/empty, whitespace-only, code-only Markdown, front-matter-only Markdown.
    let cases: [(&str, &[u8]); 5] = [
        ("empty.txt", b""),
        ("ws.txt", b"   \n\n \t \n  "),
        ("code.md", b"```rust\nfn main() {}\n```\n"),
        ("front.md", b"---\ntitle: nothing\n---\n"),
        ("bom.txt", b"\xEF\xBB\xBF"),
    ];
    for (name, bytes) in cases {
        write_bytes(work.path(), name, bytes);
        let out = ready.nar().log(&log).arg(name).dir(work.path()).run();
        out.assert_error(
            3,
            "script-empty",
            &format!("{name} has no speakable text after conversion"),
            "add text outside code blocks and front matter",
        );
    }
    assert!(wav_or_tmp_files(work.path()).is_empty(), "no .wav file may be created");
    assert!(!log.exists(), "no worker may start");
}

// ---------------------------------------------------------------------------------------
// Supported inputs generate audio and send the right text
// ---------------------------------------------------------------------------------------

// [Scenario: Supported file | docs/requirements FR-001 | FR-001 | UR-001 | NFR-008]
#[test]
fn supported_extensions_in_any_case_generate_audio() {
    let ready = Ready::new();
    let work = scratch();
    for name in ["story.txt", "story2.md", "story3.markdown", "STORY4.TXT", "Story5.Md", "story6.MARKDOWN"] {
        let script = write_text(work.path(), name, "Hello there. This is a script.");
        let out = ready.nar().arg(&script).run();
        out.assert_code(0);
        let stem = Path::new(name).file_stem().unwrap().to_string_lossy().into_owned();
        let wav = work.path().join(format!("{stem}.wav"));
        assert!(wav.is_file(), "{name} -> {}", wav.display());
        assert_output_wav_format(&wav);
    }
}

// [Scenario: Supported file | FR-001]
#[test]
fn script_path_with_spaces_and_non_ascii_characters_works() {
    let ready = Ready::new();
    let work = scratch();
    let folder = work.path().join("my scripts é");
    std::fs::create_dir(&folder).unwrap();
    let script = write_text(&folder, "my story é.txt", "Hello there.");
    let out = ready.nar().arg(&script).run();
    out.assert_code(0);
    let expected = abs(&folder.join("my story é.wav"));
    assert!(expected.is_file());
    assert_eq!(out.stdout.trim_end(), expected.display().to_string());
}

// [Scenario: BOM accepted + BOM not sent | spec-narrate-generation Req 5 | FR-001]
#[test]
fn byte_order_mark_never_reaches_the_worker() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    for name in ["story.txt", "story.md"] {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"Hello there.");
        let script = write_bytes(work.path(), name, &bytes);
        let _ = std::fs::remove_file(&log);
        let out = ready.nar().log(&log).arg(&script).run();
        out.assert_code(0);
        let texts = synth_texts(&log);
        assert!(!texts.is_empty());
        for t in &texts {
            assert!(!t.contains('\u{FEFF}'), "{name}: synth text contains a BOM: {t:?}");
        }
        assert_eq!(texts.join(" "), "Hello there.");
    }
}

// [Scenario: Markdown converted | docs/requirements FR-002 | FR-002 | UR-001]
#[test]
fn markdown_is_converted_before_it_reaches_the_worker() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let md = "# Chapter One\n\nThis is **bold** text with a [docs](https://x.y) link.\n\n```python\nprint(\"hidden\")\n```\n\nSecond paragraph.\n";
    let script = write_text(work.path(), "doc.md", md);
    ready.nar().log(&log).arg(&script).run().assert_code(0);
    let text = all_synth_text(&log);
    for must in ["Chapter One", "bold", "docs"] {
        assert!(text.contains(must), "text sent to the model must contain {must:?}: {text}");
    }
    for forbidden in ["**", "https://x.y", "print", "hidden"] {
        assert!(!text.contains(forbidden), "text sent to the model must not contain {forbidden:?}: {text}");
    }
}

// [Scenario: Other elements | docs/requirements FR-002 | FR-002]
#[test]
fn markdown_front_matter_image_list_and_inline_code_are_converted() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let md = "---\ntitle: Secret\n---\n![a cat](c.png) and `code` here.\n\n- one\n- two\n";
    let script = write_text(work.path(), "other.md", md);
    ready.nar().log(&log).arg(&script).run().assert_code(0);
    let text = all_synth_text(&log);
    for must in ["a cat", "one", "code"] {
        assert!(text.contains(must), "must contain {must:?}: {text}");
    }
    for forbidden in ["Secret", "title", "c.png", "- ", "`"] {
        assert!(!text.contains(forbidden), "must not contain {forbidden:?}: {text}");
    }
}

// [Scenario: Code-only Markdown | docs/requirements FR-002 | FR-002 | FR-004]
#[test]
fn code_only_markdown_fails_with_script_empty() {
    let ready = Ready::new();
    let work = scratch();
    let script = write_text(work.path(), "only-code.md", "~~~\nfn main() {}\n~~~\n");
    let out = ready.nar().arg(&script).run();
    out.assert_code(3);
    out.assert_stderr_contains("error[script-empty]");
}

// [Scenario: Tag preserved + Tag in Markdown | docs/requirements FR-003 | FR-003]
#[test]
fn known_tags_reach_the_worker_unchanged_in_text_and_markdown() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let txt = write_text(work.path(), "tag.txt", "Well [chuckle] that worked.");
    ready.nar().log(&log).arg(&txt).run().assert_code(0);
    assert!(all_synth_text(&log).contains("Well [chuckle] that worked."));

    let _ = std::fs::remove_file(&log);
    let md = write_text(work.path(), "tag.md", "Say [laugh] now, then [clear throat] go.");
    ready.nar().log(&log).arg(&md).run().assert_code(0);
    let text = all_synth_text(&log);
    assert!(text.contains("[laugh]") && text.contains("[clear throat]"), "{text}");
}

// [Scenario: Unknown or malformed tag | docs/requirements FR-003 | FR-003]
#[test]
fn unknown_and_unclosed_tags_pass_through_and_do_not_fail_the_run() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    for name in ["bad-tags.txt", "bad-tags.md"] {
        let script = write_text(work.path(), name, "Try [dance] and an unclosed [laugh here.");
        let _ = std::fs::remove_file(&log);
        let out = ready.nar().log(&log).arg(&script).run();
        out.assert_code(0);
        assert!(all_synth_text(&log).contains("Try [dance] and an unclosed [laugh here."), "{name}");
    }
}

// [Scenario: Plain text as-is | docs/requirements FR-003 | FR-003]
#[test]
fn plain_text_markup_is_not_interpreted() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let script = write_text(work.path(), "plain.txt", "# Not a heading with **stars** and [a](b).");
    ready.nar().log(&log).arg(&script).run().assert_code(0);
    assert_eq!(all_synth_text(&log), "# Not a heading with **stars** and [a](b).");
}

// [Scenario: Non-ASCII text round-trips | spec-narrate-worker Req 1 | FR-001]
#[test]
fn non_ascii_text_reaches_the_worker_exactly() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let script = write_text(work.path(), "cafe.txt", "Café, naïve [laugh]");
    ready.nar().log(&log).arg(&script).run().assert_code(0);
    assert_eq!(synth_texts(&log), vec!["Café, naïve [laugh]".to_string()]);
}

// ---------------------------------------------------------------------------------------
// FR-015: generation before setup
// ---------------------------------------------------------------------------------------

// [Scenario: Not set up | spec-narrate-generation Req 4 / docs/requirements FR-015 | FR-015 | NFR-008]
#[test]
fn empty_data_directory_exits_4_before_reading_the_script_or_starting_a_worker() {
    let data = scratch();
    let work = scratch();
    write_text(work.path(), "script.txt", "Hello there.");
    let log = work.path().join("worker.log");
    let out = Nar::new(data.path()).worker().log(&log).arg("script.txt").dir(work.path()).run();
    out.assert_code(4);
    let cause = out
        .find_stderr_prefix("error[not-set-up]: setup is incomplete: ")
        .unwrap_or_else(|| panic!("{}", out.context()));
    assert!(cause.contains("environment"), "{cause}");
    assert!(out.has_stderr_line("  fix: run narrate setup"), "{}", out.context());
    out.assert_stderr_contains("narrate setup");
    assert!(is_empty_dir(data.path()), "the scratch directory must remain empty");
    assert!(!log.exists(), "no worker may start");
    assert!(wav_or_tmp_files(work.path()).is_empty());
}

// [Requirement 4: readiness before reading the script | spec-narrate-generation | FR-015]
#[test]
fn readiness_is_checked_before_the_script_is_decoded_or_checked_for_content() {
    let data = scratch();
    let work = scratch();
    write_bytes(work.path(), "bad.txt", &[0xFF]);
    write_bytes(work.path(), "empty.txt", b"");
    for name in ["bad.txt", "empty.txt"] {
        let out = Nar::new(data.path()).worker().arg(name).dir(work.path()).run();
        out.assert_code(4);
        out.assert_stderr_contains("error[not-set-up]");
    }
}

// [Preconditions order: 1 to 7 before the readiness check | spec-narrate-generation | FR-015 | FR-001]
#[test]
fn file_problems_are_reported_before_setup_problems() {
    let data = scratch();
    let work = scratch();
    let out = Nar::new(data.path()).worker().arg("nofile.txt").dir(work.path()).run();
    out.assert_code(3);
    out.assert_stderr_contains("error[input-not-found]");
}

// [Scenario: Ready | spec-narrate-generation Req 4 | FR-015]
#[test]
fn a_ready_directory_does_not_mention_setup() {
    let ready = Ready::new();
    let work = scratch();
    let script = write_text(work.path(), "script.txt", "Hello there.");
    let out = ready.nar().arg(&script).run();
    out.assert_code(0);
    out.assert_stderr_lacks("narrate setup");
}

// [Requirement 7: readiness report lists every missing component | spec-narrate-setup | FR-015 | FR-008]
#[test]
fn not_set_up_lists_every_missing_component_in_table_order() {
    let ready = Ready::new();
    let work = scratch();
    write_text(work.path(), "script.txt", "Hello there.");
    ready.remove("models/chatterbox-turbo/conds.pt");
    ready.remove("voices/male.wav");
    let log = work.path().join("worker.log");
    let out = ready.nar().log(&log).arg("script.txt").dir(work.path()).run();
    out.assert_error(
        4,
        "not-set-up",
        "setup is incomplete: turbo weights missing (conds.pt), male clip missing",
        "run narrate setup",
    );
    assert!(!log.exists());
}

// [Requirement 7: setup record | spec-narrate-setup | FR-015 | FR-017]
#[test]
fn missing_or_unreadable_setup_record_gives_not_set_up() {
    let work = scratch();
    write_text(work.path(), "script.txt", "Hello there.");
    // Missing setup.json.
    let ready = Ready::new();
    std::fs::remove_file(ready.path().join("setup.json")).unwrap();
    let out = ready.nar().arg("script.txt").dir(work.path()).run();
    out.assert_code(4);
    assert!(
        out.stderr.lines().any(|l| l.starts_with("error[not-set-up]: setup is incomplete: ") && l.contains("setup record missing")),
        "{}",
        out.context()
    );
    // Unreadable (not JSON) setup.json.
    std::fs::write(ready.path().join("setup.json"), "this is not json").unwrap();
    let out = ready.nar().arg("script.txt").dir(work.path()).run();
    out.assert_code(4);
    assert!(out.stderr.contains("setup record missing"), "{}", out.context());
}

// [Scenario: Pin-set mismatch | spec-narrate-generation Req 4 | FR-017 | FR-015 | NFR-007]
#[test]
fn pin_set_mismatch_exits_4_with_both_ids() {
    let ready = Ready::new();
    let work = scratch();
    write_text(work.path(), "script.txt", "Hello there.");
    let zeros = "0".repeat(64);
    std::fs::write(ready.path().join("setup.json"), setup_json(&zeros)).unwrap();
    let log = work.path().join("worker.log");
    let out = ready.nar().log(&log).arg("script.txt").dir(work.path()).run();
    out.assert_error(
        4,
        "pin-mismatch",
        &format!(
            "the data directory {} was set up with a different pin set (recorded {zeros}, this binary {})",
            ready.d().display(),
            ready.id
        ),
        "run narrate setup",
    );
    out.assert_stderr_contains("narrate setup");
    assert!(!log.exists(), "no worker may start and nothing may be downloaded");
}

// [Requirement 7: unknown fields ignored | spec-narrate-setup | FR-017]
#[test]
fn setup_record_with_extra_fields_is_still_accepted() {
    let ready = Ready::new();
    let work = scratch();
    let script = write_text(work.path(), "script.txt", "Hello there.");
    std::fs::write(
        ready.path().join("setup.json"),
        format!(
            "{{\"format\":1,\"pin_set_id\":\"{}\",\"narrate_version\":\"0.1.0\",\"completed_unix\":1760300000,\"clips\":{{}},\"future_field\":[1,2,3]}}",
            ready.id
        ),
    )
    .unwrap();
    ready.nar().arg(&script).run().assert_code(0);
}
