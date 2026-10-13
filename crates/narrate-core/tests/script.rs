//! Black-box tests for `narrate_core::script` and the plain-text converter.
//!
//! Source of truth: specs/spec-narrate-text-pipeline.md (Requirements 1 to 3, RULE-001,
//! RULE-002) and the `load_script` contract in the same file. No implementation was read.
//!
//! Traceability: every test carries `[Scenario | spec | FR/UR]`.

use std::path::{Path, PathBuf};

use narrate_core::script::{decode_utf8, load_script, ScriptError, ScriptKind};
use narrate_core::spoken::{plain_to_spoken, Pause, SpokenScript};
use tempfile::TempDir;

fn pairs(script: &SpokenScript) -> Vec<(String, Pause)> {
    script
        .segments
        .iter()
        .map(|s| (s.text.clone(), s.pause_after))
        .collect()
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.path().join(name);
    std::fs::write(&p, bytes).expect("write fixture");
    p
}

fn assert_is_error<E: std::error::Error + std::fmt::Debug + std::fmt::Display>() {}

// ---------------------------------------------------------------------------------------
// Requirement 1: ScriptKind::from_path
// ---------------------------------------------------------------------------------------

// [Scenario: Supported extensions | spec-narrate-text-pipeline | FR-001 | UR-001 | RULE-001]
#[test]
fn from_path_maps_supported_extensions_case_insensitively() {
    // Valid: the three spec examples.
    assert_eq!(
        ScriptKind::from_path(Path::new("a.md")).unwrap(),
        ScriptKind::Markdown
    );
    assert_eq!(
        ScriptKind::from_path(Path::new("b.MARKDOWN")).unwrap(),
        ScriptKind::Markdown
    );
    assert_eq!(
        ScriptKind::from_path(Path::new("c.Txt")).unwrap(),
        ScriptKind::PlainText
    );
}

// [Scenario: Supported extensions | spec-narrate-text-pipeline | FR-001 | RULE-001]
#[test]
fn from_path_accepts_markdown_long_form_uppercase_txt_and_directories_in_path() {
    // Valid variations: `.markdown`, `NOTES.TXT` (RULE-001 example), nested directories.
    assert_eq!(
        ScriptKind::from_path(Path::new("x.markdown")).unwrap(),
        ScriptKind::Markdown
    );
    assert_eq!(
        ScriptKind::from_path(Path::new("NOTES.TXT")).unwrap(),
        ScriptKind::PlainText
    );
    assert_eq!(
        ScriptKind::from_path(Path::new(r"C:\some dir\sub.dir\story.md")).unwrap(),
        ScriptKind::Markdown
    );
    assert_eq!(
        ScriptKind::from_path(Path::new("a.b.txt")).unwrap(),
        ScriptKind::PlainText
    );
}

// [Scenario: Unsupported extension | spec-narrate-text-pipeline | FR-001]
#[test]
fn from_path_rejects_other_or_missing_extension_with_the_given_path() {
    // Invalid: the three spec examples.
    for name in ["story.docx", "story", "story.md.bak"] {
        match ScriptKind::from_path(Path::new(name)) {
            Err(ScriptError::UnsupportedExtension { path }) => {
                assert_eq!(path, PathBuf::from(name), "path carried for {name}");
            }
            other => panic!("{name}: expected UnsupportedExtension, got {other:?}"),
        }
    }
}

// [Scenario: Unsupported extension | spec-narrate-text-pipeline | FR-001]
#[test]
fn from_path_rejects_lookalike_and_empty_extensions() {
    // Boundary / injection-shaped: near-miss extensions must not be accepted.
    for name in ["story.mdx", "story.txt.docx", "story.text", "story.", "notes.md ", "story.m d"] {
        assert!(
            matches!(
                ScriptKind::from_path(Path::new(name)),
                Err(ScriptError::UnsupportedExtension { .. })
            ),
            "{name:?} must be rejected"
        );
    }
}

// [Scenario: Supported extensions | spec-narrate-text-pipeline | FR-001]
#[test]
fn from_path_checks_only_the_extension_not_existence() {
    let p = Path::new(r"Z:\this\path\does\not\exist\ghost.txt");
    assert_eq!(ScriptKind::from_path(p).unwrap(), ScriptKind::PlainText);
}

// ---------------------------------------------------------------------------------------
// Requirement 2: decode_utf8
// ---------------------------------------------------------------------------------------

// [Scenario: BOM removed | spec-narrate-text-pipeline | FR-001 | RULE-002]
#[test]
fn decode_utf8_removes_one_leading_bom() {
    let text = decode_utf8(Path::new("a.txt"), &[0xEF, 0xBB, 0xBF, 0x48, 0x69]).unwrap();
    assert_eq!(text, "Hi");
    assert!(!text.contains('\u{FEFF}'));
}

// [Scenario: BOM removed | spec-narrate-text-pipeline | FR-001 | RULE-002]
#[test]
fn decode_utf8_removes_exactly_one_bom_and_keeps_a_second() {
    // Boundary: "any other U+FEFF stays".
    let text = decode_utf8(
        Path::new("a.txt"),
        &[0xEF, 0xBB, 0xBF, 0xEF, 0xBB, 0xBF, 0x48],
    )
    .unwrap();
    assert_eq!(text, "\u{FEFF}H");
}

// [Scenario: BOM removed | spec-narrate-text-pipeline | FR-001 | RULE-002]
#[test]
fn decode_utf8_keeps_a_bom_that_is_not_leading() {
    let mut bytes = b"a".to_vec();
    bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    bytes.push(b'b');
    assert_eq!(
        decode_utf8(Path::new("a.txt"), &bytes).unwrap(),
        "a\u{FEFF}b"
    );
}

// [Scenario: BOM removed | spec-narrate-text-pipeline | FR-001]
#[test]
fn decode_utf8_handles_empty_bom_only_and_multibyte_input() {
    // Null/empty: no bytes, and a BOM with nothing after it.
    assert_eq!(decode_utf8(Path::new("a.txt"), &[]).unwrap(), "");
    assert_eq!(decode_utf8(Path::new("a.txt"), &[0xEF, 0xBB, 0xBF]).unwrap(), "");
    // Valid multibyte text round-trips unchanged.
    let original = "Café naïve 日本語 😀";
    assert_eq!(
        decode_utf8(Path::new("a.txt"), original.as_bytes()).unwrap(),
        original
    );
}

// [Scenario: Invalid byte | spec-narrate-text-pipeline | FR-001]
#[test]
fn decode_utf8_reports_offset_of_first_invalid_byte_and_names_file() {
    let err = decode_utf8(Path::new("bad.txt"), &[0x61, 0x62, 0xFF, 0x63]).unwrap_err();
    match &err {
        ScriptError::InvalidUtf8 { path, offset } => {
            assert_eq!(path, &PathBuf::from("bad.txt"));
            assert_eq!(*offset, 2);
        }
        other => panic!("expected InvalidUtf8, got {other:?}"),
    }
    assert!(matches!(err, ScriptError::InvalidUtf8 { offset: 2, .. }));
    let shown = err.to_string();
    assert!(shown.contains("bad.txt"), "Display must name the file: {shown}");
    assert!(shown.contains("UTF-8"), "Display must name the encoding: {shown}");
}

// [Scenario: Invalid byte | spec-narrate-text-pipeline | FR-001]
#[test]
fn decode_utf8_offset_table_counts_from_start_of_original_bytes() {
    // Boundary: first position, later positions, after a BOM (offset counts the BOM bytes),
    // overlong encoding, surrogate encoding, stray continuation byte, several bad bytes.
    let cases: &[(&[u8], usize)] = &[
        (&[0xFF], 0),
        (&[0x61, 0xFF], 1),
        (&[0xEF, 0xBB, 0xBF, 0x61, 0xFF], 4),
        (&[0x61, 0xC0, 0x80], 1),
        (&[0x61, 0xED, 0xA0, 0x80], 1),
        (&[0x68, 0x69, 0x80], 2),
        (&[0x61, 0xFF, 0xFE], 1),
    ];
    for (bytes, expected) in cases {
        let err = decode_utf8(Path::new("bad.txt"), bytes).unwrap_err();
        match err {
            ScriptError::InvalidUtf8 { offset, .. } => {
                assert_eq!(offset, *expected, "bytes {bytes:02X?}");
            }
            other => panic!("bytes {bytes:02X?}: expected InvalidUtf8, got {other:?}"),
        }
    }
}

// [Scenario: Invalid byte | spec-narrate-text-pipeline | FR-001]
#[test]
fn script_error_implements_error_debug_and_display() {
    assert_is_error::<ScriptError>();
}

// ---------------------------------------------------------------------------------------
// load_script
// ---------------------------------------------------------------------------------------

// [Scenario: Unsupported extension | spec-narrate-text-pipeline | FR-001]
#[test]
fn load_script_rejects_extension_before_checking_existence() {
    let dir = tempfile::tempdir().unwrap();
    // Missing file with a bad extension: UnsupportedExtension wins over NotFound.
    let missing = dir.path().join("nofile.docx");
    assert!(matches!(
        load_script(&missing),
        Err(ScriptError::UnsupportedExtension { .. })
    ));
    // Existing file with a bad extension.
    let existing = write(&dir, "story.docx", b"hello");
    match load_script(&existing) {
        Err(ScriptError::UnsupportedExtension { path }) => assert_eq!(path, existing),
        other => panic!("expected UnsupportedExtension, got {other:?}"),
    }
}

// [Scenario: Missing file | spec-narrate-generation Req 1 | FR-001]
#[test]
fn load_script_reports_not_found_with_the_given_path() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nofile.txt");
    match load_script(&missing) {
        Err(ScriptError::NotFound { path }) => assert_eq!(path, missing),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

// [Preconditions: path is a file | spec-narrate-text-pipeline | FR-001]
#[test]
fn load_script_reports_not_a_file_for_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("folder.txt");
    std::fs::create_dir(&folder).unwrap();
    match load_script(&folder) {
        Err(ScriptError::NotAFile { path }) => assert_eq!(path, folder),
        other => panic!("expected NotAFile, got {other:?}"),
    }
}

// [Scenario: Paragraphs and pass-through | spec-narrate-text-pipeline | FR-001 | UR-001]
#[test]
fn load_script_converts_a_plain_text_file_into_paragraph_segments() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(&dir, "story.txt", b"First para.\n\nSecond para.\n");
    let script = load_script(&p).unwrap();
    assert_eq!(
        pairs(&script),
        vec![
            ("First para.".to_string(), Pause::Paragraph),
            ("Second para.".to_string(), Pause::Paragraph)
        ]
    );
}

// [Scenario: Supported extensions | spec-narrate-text-pipeline | FR-001 | RULE-001]
#[test]
fn load_script_accepts_uppercase_extensions_and_converts_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let md = write(&dir, "INTRO.MD", b"# Title\n\nBody text.\n");
    let script = load_script(&md).unwrap();
    assert_eq!(
        pairs(&script),
        vec![
            ("Title".to_string(), Pause::Heading),
            ("Body text.".to_string(), Pause::Paragraph)
        ]
    );
    let txt = write(&dir, "NOTES.TXT", b"Upper case extension.");
    assert_eq!(
        pairs(&load_script(&txt).unwrap()),
        vec![("Upper case extension.".to_string(), Pause::Paragraph)]
    );
}

// [Scenario: BOM removed | spec-narrate-text-pipeline Req 2 | FR-001 | RULE-002]
#[test]
fn load_script_strips_the_bom_so_it_never_reaches_the_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"Hello there.");
    let p = write(&dir, "story.txt", &bytes);
    let script = load_script(&p).unwrap();
    assert_eq!(
        pairs(&script),
        vec![("Hello there.".to_string(), Pause::Paragraph)]
    );
    assert!(!script.spoken_text().contains('\u{FEFF}'));
}

// [Scenario: Invalid byte | spec-narrate-text-pipeline | FR-001]
#[test]
fn load_script_reports_invalid_utf8_with_offset_and_path() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(&dir, "bad.txt", &[0x61, 0x62, 0xFF, 0x63]);
    match load_script(&p) {
        Err(ScriptError::InvalidUtf8 { path, offset }) => {
            assert_eq!(path, p);
            assert_eq!(offset, 2);
        }
        other => panic!("expected InvalidUtf8, got {other:?}"),
    }
}

// [Scenario: Whitespace only / Empty file | spec-narrate-text-pipeline | FR-004]
#[test]
fn load_script_returns_empty_for_zero_bytes_and_whitespace_only_text() {
    let dir = tempfile::tempdir().unwrap();
    // Null/empty and whitespace-only inputs.
    for (name, bytes) in [
        ("empty.txt", &b""[..]),
        ("ws.txt", &b"  \n\t\n  "[..]),
        ("crlf.txt", &b"\r\n\r\n   \r\n"[..]),
        ("bom-only.txt", &[0xEF, 0xBB, 0xBF][..]),
        ("empty.md", &b""[..]),
    ] {
        let p = write(&dir, name, bytes);
        match load_script(&p) {
            Err(ScriptError::Empty { path }) => assert_eq!(path, p, "{name}"),
            other => panic!("{name}: expected Empty, got {other:?}"),
        }
    }
}

// [Scenario: Code-only document | spec-narrate-text-pipeline | FR-002 | FR-004]
#[test]
fn load_script_returns_empty_for_a_markdown_file_with_only_code() {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in [
        ("code-backtick.md", "```rust\nfn main() {}\n```\n"),
        ("code-tilde.md", "~~~python\nprint('x')\n~~~\n"),
        ("code-indented.md", "    indented code only\n"),
        ("front-only.md", "---\ntitle: x\n---\n"),
    ] {
        let p = write(&dir, name, body.as_bytes());
        match load_script(&p) {
            Err(ScriptError::Empty { path }) => assert_eq!(path, p, "{name}"),
            other => panic!("{name}: expected Empty, got {other:?}"),
        }
    }
}

// [Preconditions: read failure | spec-narrate-text-pipeline | FR-001]
#[test]
fn load_script_reports_io_when_the_file_cannot_be_read() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let p = write(&dir, "locked.txt", b"locked content");
    // Hold the file with share mode 0: metadata still works, reading fails (sharing violation).
    let _guard = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&p)
        .expect("open exclusively");
    match load_script(&p) {
        Err(ScriptError::Io { path, .. }) => assert_eq!(path, p),
        other => panic!("expected Io, got {other:?}"),
    }
}

// [Public API re-exports | spec-narrate-text-pipeline | UR-001]
#[test]
fn script_items_are_re_exported_at_the_crate_root() {
    let kind: narrate_core::ScriptKind = narrate_core::ScriptKind::from_path(Path::new("a.md")).unwrap();
    assert_eq!(kind, narrate_core::ScriptKind::Markdown);
    let _ = narrate_core::decode_utf8(Path::new("a.txt"), b"x").unwrap();
    let _: fn(&Path) -> Result<narrate_core::SpokenScript, narrate_core::ScriptError> =
        narrate_core::load_script;
}

// ---------------------------------------------------------------------------------------
// Requirement 3: plain_to_spoken
// ---------------------------------------------------------------------------------------

// [Scenario: Paragraphs and pass-through | spec-narrate-text-pipeline | FR-003 | UR-001]
#[test]
fn plain_to_spoken_splits_at_blank_lines_and_passes_markup_and_tags_through() {
    let script =
        plain_to_spoken("Well [chuckle] that **worked**.\r\nNext line.\r\n\r\n\r\nSecond  para.");
    assert_eq!(
        pairs(&script),
        vec![
            (
                "Well [chuckle] that **worked**. Next line.".to_string(),
                Pause::Paragraph
            ),
            ("Second para.".to_string(), Pause::Paragraph)
        ]
    );
}

// [Scenario: Whitespace only | spec-narrate-text-pipeline | FR-004]
#[test]
fn plain_to_spoken_returns_no_segments_for_whitespace_only_text() {
    let script = plain_to_spoken("  \n\t\n  ");
    assert!(script.segments.is_empty());
    assert!(script.is_empty());
    // Null/empty equivalent.
    assert!(plain_to_spoken("").is_empty());
}

// [Requirement 3: blank lines | spec-narrate-text-pipeline | FR-003]
#[test]
fn plain_to_spoken_treats_whitespace_only_lines_as_blank_and_drops_empty_paragraphs() {
    let script = plain_to_spoken("\n\na\n   \t \nb\n\n\n\n");
    assert_eq!(
        pairs(&script),
        vec![
            ("a".to_string(), Pause::Paragraph),
            ("b".to_string(), Pause::Paragraph)
        ]
    );
}

// [Scenario: Paragraphs and pass-through | spec-narrate-text-pipeline | FR-003]
#[test]
fn plain_to_spoken_does_not_interpret_markdown_syntax() {
    let script = plain_to_spoken("# Not a heading\n\n- not a list\n\n[docs](https://x.y) stays");
    assert_eq!(
        pairs(&script),
        vec![
            ("# Not a heading".to_string(), Pause::Paragraph),
            ("- not a list".to_string(), Pause::Paragraph),
            ("[docs](https://x.y) stays".to_string(), Pause::Paragraph)
        ]
    );
}

// [Requirement 3: no other character changes | spec-narrate-text-pipeline | FR-003 | FR-001]
#[test]
fn plain_to_spoken_keeps_non_ascii_text_unchanged() {
    let script = plain_to_spoken("Café, naïve [laugh]");
    assert_eq!(
        pairs(&script),
        vec![("Café, naïve [laugh]".to_string(), Pause::Paragraph)]
    );
}
