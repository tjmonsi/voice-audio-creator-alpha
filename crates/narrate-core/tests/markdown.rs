//! Black-box tests for the Markdown-to-speech rules (RULE-010 to RULE-024) and the
//! worked examples of specs/spec-narrate-text-pipeline.md (Requirement 4).
//!
//! No implementation was read. Expected segments are copied from the spec Scenarios; extra
//! cases are derived only from the business-rule table.

use narrate_core::script::ScriptKind;
use narrate_core::spoken::{
    markdown_to_spoken, normalize_whitespace, to_spoken, Pause, Segment, SpokenScript,
};

use Pause::{Cell, Heading, Paragraph};

fn pairs(script: &SpokenScript) -> Vec<(String, Pause)> {
    script
        .segments
        .iter()
        .map(|s| (s.text.clone(), s.pause_after))
        .collect()
}

fn expect(items: &[(&str, Pause)]) -> Vec<(String, Pause)> {
    items.iter().map(|(t, p)| (t.to_string(), *p)).collect()
}

fn md(text: &str) -> Vec<(String, Pause)> {
    pairs(&markdown_to_spoken(text))
}

// ---------------------------------------------------------------------------------------
// Worked examples from the spec
// ---------------------------------------------------------------------------------------

// [Scenario: Heading, emphasis, link, code block | spec-narrate-text-pipeline | FR-002 | UR-001]
#[test]
fn heading_emphasis_link_and_tilde_code_block() {
    let input = "# Chapter One\n\nThis is **bold** text with a [docs](https://x.y) link.\n\n~~~python\nprint(\"hidden\")\n~~~\n\nSecond paragraph.\n";
    let script = markdown_to_spoken(input);
    assert_eq!(
        pairs(&script),
        expect(&[
            ("Chapter One", Heading),
            ("This is bold text with a docs link.", Paragraph),
            ("Second paragraph.", Paragraph),
        ])
    );
    let spoken = script.spoken_text();
    for forbidden in ["**", "https://x.y", "print", "hidden"] {
        assert!(!spoken.contains(forbidden), "spoken text must not contain {forbidden:?}: {spoken}");
    }
}

// [Scenario: Heading, emphasis, link, code block | spec-narrate-text-pipeline | FR-002]
#[test]
fn backtick_fence_behaves_like_tilde_fence() {
    let input = "# Chapter One\n\nThis is **bold** text with a [docs](https://x.y) link.\n\n```python\nprint(\"hidden\")\n```\n\nSecond paragraph.\n";
    assert_eq!(
        md(input),
        expect(&[
            ("Chapter One", Heading),
            ("This is bold text with a docs link.", Paragraph),
            ("Second paragraph.", Paragraph),
        ])
    );
}

// [Scenario: Front matter, image, inline code, list | spec-narrate-text-pipeline | FR-002]
#[test]
fn front_matter_image_inline_code_and_list() {
    let input = "---\ntitle: Secret\n---\n![a cat](c.png) and `code` here.\n\n- one\n- two\n";
    let script = markdown_to_spoken(input);
    assert_eq!(
        pairs(&script),
        expect(&[
            ("a cat and code here.", Paragraph),
            ("one", Paragraph),
            ("two", Paragraph),
        ])
    );
    let spoken = script.spoken_text();
    for forbidden in ["Secret", "title", "c.png", "- ", "`"] {
        assert!(!spoken.contains(forbidden), "spoken text must not contain {forbidden:?}: {spoken}");
    }
}

// [Scenario: Table | spec-narrate-text-pipeline | FR-002 | RULE-021]
#[test]
fn table_rows_in_order_with_cell_and_paragraph_pauses() {
    let input = "| Name | Age |\n|------|-----|\n| Ann  | 30  |\n";
    assert_eq!(
        md(input),
        expect(&[
            ("Name", Cell),
            ("Age", Paragraph),
            ("Ann", Cell),
            ("30", Paragraph),
        ])
    );
}

// [Scenario: Block quote and nested list | spec-narrate-text-pipeline | FR-002 | RULE-019 | RULE-020]
#[test]
fn block_quote_and_nested_list() {
    let input = "> Quoted line one\n> continues here.\n\n1. First\n   - nested\n2. Second\n";
    assert_eq!(
        md(input),
        expect(&[
            ("Quoted line one continues here.", Paragraph),
            ("First", Paragraph),
            ("nested", Paragraph),
            ("Second", Paragraph),
        ])
    );
}

// [Scenario: Raw HTML, comments, entities, autolinks, footnotes | spec-narrate-text-pipeline | FR-002 | RULE-015 | RULE-022 | RULE-023]
#[test]
fn raw_html_comments_entities_autolinks_and_footnotes() {
    let input = "<div class=\"note\">Hello <b>world</b></div>\n\nText with <span>inline</span> html &amp; more[^1]. See <https://example.com> now.\n\n<!-- secret -->\n\n[^1]: Footnote text.\n";
    assert_eq!(
        md(input),
        expect(&[
            ("Hello world", Paragraph),
            ("Text with inline html & more. See now.", Paragraph),
        ])
    );
}

// [Scenario: Setext heading and thematic break | spec-narrate-text-pipeline | FR-002 | RULE-011 | RULE-023]
#[test]
fn setext_heading_and_thematic_break() {
    assert_eq!(
        md("Title\n=====\n\n***\n\nEnd."),
        expect(&[("Title", Heading), ("End.", Paragraph)])
    );
}

// ---------------------------------------------------------------------------------------
// RULE-010: front matter
// ---------------------------------------------------------------------------------------

// [RULE-010 | spec-narrate-text-pipeline | FR-002]
#[test]
fn front_matter_closed_by_dashes_is_skipped() {
    assert_eq!(md("---\ntitle: x\n---\nHi"), expect(&[("Hi", Paragraph)]));
}

// [RULE-010 | spec-narrate-text-pipeline | FR-002]
#[test]
fn front_matter_closed_by_three_dots_is_skipped() {
    assert_eq!(md("---\ntitle: x\n...\nHi"), expect(&[("Hi", Paragraph)]));
}

// [RULE-010 | spec-narrate-text-pipeline | FR-002]
#[test]
fn front_matter_opening_line_may_have_trailing_spaces() {
    assert_eq!(md("---   \ntitle: x\n---\nHi"), expect(&[("Hi", Paragraph)]));
}

// [RULE-010 | spec-narrate-text-pipeline | FR-002]
#[test]
fn front_matter_with_multiple_keys_and_crlf_is_skipped() {
    assert_eq!(
        md("---\r\ntitle: x\r\ntags: [a, b]\r\n---\r\nHi there"),
        expect(&[("Hi there", Paragraph)])
    );
}

// [RULE-010 | spec-narrate-text-pipeline | FR-002]
#[test]
fn unclosed_front_matter_is_not_front_matter() {
    // Invalid GIVEN: no closing line, so the file has no front matter. The opening `---` is a
    // thematic break (no text) and the rest is an ordinary paragraph.
    assert_eq!(
        md("---\ntitle: x\nHi"),
        expect(&[("title: x Hi", Paragraph)])
    );
}

// ---------------------------------------------------------------------------------------
// RULE-011, RULE-012: headings and paragraphs
// ---------------------------------------------------------------------------------------

// [RULE-011 | spec-narrate-text-pipeline | FR-002]
#[test]
fn atx_headings_drop_markers_and_closing_hashes() {
    assert_eq!(md("## Intro ##"), expect(&[("Intro", Heading)]));
    assert_eq!(md("# One\n\n### Three ###\n"), expect(&[("One", Heading), ("Three", Heading)]));
    assert_eq!(md("###### Six"), expect(&[("Six", Heading)]));
}

// [RULE-011 | spec-narrate-text-pipeline | FR-002]
#[test]
fn setext_level_two_heading_is_a_heading() {
    assert_eq!(md("Sub title\n---------\n\nBody."), expect(&[("Sub title", Heading), ("Body.", Paragraph)]));
}

// [RULE-011 | spec-narrate-text-pipeline | FR-002 | RULE-013]
#[test]
fn heading_text_has_inline_markup_removed() {
    assert_eq!(md("# A *fine* [day](https://x.y)"), expect(&[("A fine day", Heading)]));
}

// [RULE-012, RULE-023 | spec-narrate-text-pipeline | FR-002]
#[test]
fn soft_and_hard_line_breaks_become_a_space() {
    assert_eq!(md("line one\nline two"), expect(&[("line one line two", Paragraph)]));
    assert_eq!(md("hard  \nbreak"), expect(&[("hard break", Paragraph)]));
    assert_eq!(md("slash\\\nbreak"), expect(&[("slash break", Paragraph)]));
    assert_eq!(
        md("# T\r\n\r\nBody\r\nmore\r\n"),
        expect(&[("T", Heading), ("Body more", Paragraph)])
    );
}

// [RULE-012 | spec-narrate-text-pipeline | FR-002]
#[test]
fn each_paragraph_is_one_segment() {
    assert_eq!(
        md("One.\n\nTwo.\n\n\n\nThree."),
        expect(&[("One.", Paragraph), ("Two.", Paragraph), ("Three.", Paragraph)])
    );
}

// ---------------------------------------------------------------------------------------
// RULE-013 to RULE-018: inline rules
// ---------------------------------------------------------------------------------------

// [RULE-013 | spec-narrate-text-pipeline | FR-002]
#[test]
fn emphasis_strong_and_strikethrough_markers_are_removed() {
    assert_eq!(md("~~old~~ new"), expect(&[("old new", Paragraph)]));
    assert_eq!(md("*em* and _em_"), expect(&[("em and em", Paragraph)]));
    assert_eq!(md("**strong** and __strong__"), expect(&[("strong and strong", Paragraph)]));
    assert_eq!(md("***both***"), expect(&[("both", Paragraph)]));
}

// [RULE-014 | spec-narrate-text-pipeline | FR-002]
#[test]
fn inline_link_keeps_text_and_drops_destination_and_title() {
    assert_eq!(md("[docs](https://x.y)"), expect(&[("docs", Paragraph)]));
    assert_eq!(md("[docs](https://x.y \"A title\") end"), expect(&[("docs end", Paragraph)]));
}

// [RULE-014 | spec-narrate-text-pipeline | FR-002]
#[test]
fn reference_collapsed_and_shortcut_links_keep_text_and_definitions_vanish() {
    let input = "Full [docs][ref], collapsed [ref][], shortcut [ref].\n\n[ref]: https://x.y \"Title\"\n";
    assert_eq!(
        md(input),
        expect(&[("Full docs, collapsed ref, shortcut ref.", Paragraph)])
    );
}

// [RULE-014 | spec-narrate-text-pipeline | FR-002]
#[test]
fn link_reference_definitions_alone_produce_no_text() {
    assert!(markdown_to_spoken("[ref]: https://x.y\n").is_empty());
}

// [RULE-015 | spec-narrate-text-pipeline | FR-002]
#[test]
fn autolinks_are_dropped_but_bare_urls_stay_as_text() {
    assert_eq!(md("See <https://example.com> now."), expect(&[("See now.", Paragraph)]));
    assert_eq!(md("Mail <user@host.com> today."), expect(&[("Mail today.", Paragraph)]));
    assert_eq!(md("Visit https://x.y today."), expect(&[("Visit https://x.y today.", Paragraph)]));
}

// [RULE-016 | spec-narrate-text-pipeline | FR-002]
#[test]
fn image_alt_text_is_kept_and_empty_alt_produces_nothing() {
    assert_eq!(md("![a cat](c.png)"), expect(&[("a cat", Paragraph)]));
    assert_eq!(md("Before ![](x.png) after."), expect(&[("Before after.", Paragraph)]));
    assert!(markdown_to_spoken("![](x.png)").is_empty());
}

// [RULE-017 | spec-narrate-text-pipeline | FR-002]
#[test]
fn inline_code_keeps_content_without_backticks() {
    assert_eq!(md("Set `x = 1` now."), expect(&[("Set x = 1 now.", Paragraph)]));
}

// [RULE-018 | spec-narrate-text-pipeline | FR-002]
#[test]
fn indented_and_fenced_code_blocks_are_dropped() {
    assert_eq!(
        md("Para.\n\n    code line\n\nEnd."),
        expect(&[("Para.", Paragraph), ("End.", Paragraph)])
    );
    assert_eq!(
        md("Para.\n\n```\nsecret code\n```\n\nEnd."),
        expect(&[("Para.", Paragraph), ("End.", Paragraph)])
    );
}

// ---------------------------------------------------------------------------------------
// RULE-019, RULE-020: lists and block quotes
// ---------------------------------------------------------------------------------------

// [RULE-019 | spec-narrate-text-pipeline | FR-002]
#[test]
fn all_list_marker_styles_are_removed() {
    for input in ["- a\n- b\n", "* a\n* b\n", "+ a\n+ b\n", "1. a\n2. b\n", "1) a\n2) b\n"] {
        assert_eq!(
            md(input),
            expect(&[("a", Paragraph), ("b", Paragraph)]),
            "input {input:?}"
        );
    }
}

// [RULE-019 | spec-narrate-text-pipeline | FR-002]
#[test]
fn task_list_markers_are_removed() {
    assert_eq!(
        md("- [ ] todo\n- [x] done\n- [X] Done\n"),
        expect(&[("todo", Paragraph), ("done", Paragraph), ("Done", Paragraph)])
    );
}

// [RULE-019 | spec-narrate-text-pipeline | FR-002]
#[test]
fn list_item_extra_paragraphs_and_inline_markup() {
    assert_eq!(
        md("- first\n\n  second para\n- next\n"),
        expect(&[("first", Paragraph), ("second para", Paragraph), ("next", Paragraph)])
    );
    assert_eq!(md("- **bold** item\n"), expect(&[("bold item", Paragraph)]));
}

// [RULE-019 | spec-narrate-text-pipeline | FR-002]
#[test]
fn loose_list_reads_like_a_tight_list() {
    assert_eq!(
        md("- a\n\n- b\n"),
        expect(&[("a", Paragraph), ("b", Paragraph)])
    );
}

// [RULE-020 | spec-narrate-text-pipeline | FR-002]
#[test]
fn nested_block_quotes_and_inner_blocks_follow_the_other_rules() {
    assert_eq!(md("> > inner"), expect(&[("inner", Paragraph)]));
    assert_eq!(
        md("> # Quote head\n>\n> Quote body."),
        expect(&[("Quote head", Heading), ("Quote body.", Paragraph)])
    );
}

// ---------------------------------------------------------------------------------------
// RULE-021: tables
// ---------------------------------------------------------------------------------------

// [RULE-021 | spec-narrate-text-pipeline | FR-002]
#[test]
fn table_empty_cells_are_skipped_and_last_non_empty_cell_gets_paragraph() {
    let input = "| a |   | c |\n|---|---|---|\n| x | y |   |\n";
    assert_eq!(
        md(input),
        expect(&[
            ("a", Cell),
            ("c", Paragraph),
            ("x", Cell),
            ("y", Paragraph),
        ])
    );
}

// [RULE-021 | spec-narrate-text-pipeline | FR-002]
#[test]
fn table_with_three_columns_and_inline_markup() {
    let input = "| A | B | C |\n|---|---|---|\n| **1** | `2` | [3](https://x.y) |\n";
    assert_eq!(
        md(input),
        expect(&[
            ("A", Cell),
            ("B", Cell),
            ("C", Paragraph),
            ("1", Cell),
            ("2", Cell),
            ("3", Paragraph),
        ])
    );
}

// ---------------------------------------------------------------------------------------
// RULE-022, RULE-023: HTML, entities, escapes
// ---------------------------------------------------------------------------------------

// [RULE-022 | spec-narrate-text-pipeline | FR-002]
#[test]
fn html_comments_br_script_and_style_are_handled() {
    assert_eq!(md("text <!-- hidden --> more"), expect(&[("text more", Paragraph)]));
    assert_eq!(md("line<br>two"), expect(&[("line two", Paragraph)]));
    assert_eq!(md("Hi <script>alert(1)</script> there."), expect(&[("Hi there.", Paragraph)]));
    assert_eq!(
        md("<script>\nvar a = 1;\n</script>\n\nAfter."),
        expect(&[("After.", Paragraph)])
    );
    assert_eq!(
        md("<style>\nbody { color: red }\n</style>\n\nAfter."),
        expect(&[("After.", Paragraph)])
    );
}

// [RULE-022 | spec-narrate-text-pipeline | FR-002]
#[test]
fn html_block_remaining_text_is_one_paragraph() {
    assert_eq!(md("<div>\nHello <b>world</b>\n</div>"), expect(&[("Hello world", Paragraph)]));
}

// [RULE-022 | spec-narrate-text-pipeline | FR-002]
#[test]
fn named_and_numeric_entities_are_decoded() {
    assert_eq!(
        md("Tom &amp; Jerry &lt;3 &quot;hi&quot; &#39;x&#39; &apos;y&apos; &#65;&#x42;"),
        expect(&[("Tom & Jerry <3 \"hi\" 'x' 'y' AB", Paragraph)])
    );
    // &nbsp; decodes to U+00A0, which normalization turns into a single space.
    assert_eq!(md("a&nbsp;b"), expect(&[("a b", Paragraph)]));
}

// [RULE-023 | spec-narrate-text-pipeline | FR-002]
#[test]
fn backslash_escapes_are_decoded() {
    assert_eq!(md("\\*x\\*"), expect(&[("*x*", Paragraph)]));
}

// [RULE-023 | spec-narrate-text-pipeline | FR-002]
#[test]
fn thematic_breaks_and_footnotes_produce_no_text() {
    assert_eq!(
        md("A.\n\n---\n\nB.\n\n___\n\nC."),
        expect(&[("A.", Paragraph), ("B.", Paragraph), ("C.", Paragraph)])
    );
    assert!(markdown_to_spoken("[^1]: Only a footnote definition.\n").is_empty());
}

// ---------------------------------------------------------------------------------------
// Requirement 5 through the Markdown converter (tags are verbatim; see also tags.rs)
// ---------------------------------------------------------------------------------------

// [Scenario: Tag in Markdown | spec-narrate-text-pipeline | FR-003 | RULE-024]
#[test]
fn bracketed_tags_in_headings_lists_and_tables_stay_verbatim() {
    assert_eq!(md("# Hello [sigh]"), expect(&[("Hello [sigh]", Heading)]));
    assert_eq!(md("- [laugh] ok\n"), expect(&[("[laugh] ok", Paragraph)]));
    assert_eq!(
        md("| [laugh] | b |\n|---|---|\n| c | d |\n"),
        expect(&[("[laugh]", Cell), ("b", Paragraph), ("c", Cell), ("d", Paragraph)])
    );
}

// [Scenario: Tag followed by a link target is a link | spec-narrate-text-pipeline | FR-003 | RULE-014 | RULE-024]
#[test]
fn bracketed_text_with_matching_definition_is_a_link() {
    assert_eq!(
        md("Hello [laugh] there.\n\n[laugh]: https://x.y\n"),
        expect(&[("Hello laugh there.", Paragraph)])
    );
}

// ---------------------------------------------------------------------------------------
// Input-space edges
// ---------------------------------------------------------------------------------------

// [Requirement 4: dropped empty segments | spec-narrate-text-pipeline | FR-002 | FR-004]
#[test]
fn empty_and_whitespace_only_markdown_produce_no_segments() {
    assert!(markdown_to_spoken("").is_empty());
    assert!(markdown_to_spoken("   \n\n\t\n").is_empty());
}

// [Requirement 4: malformed Markdown is not an error | spec-narrate-text-pipeline | FR-002]
#[test]
fn unbalanced_markup_is_best_effort_text_and_never_panics() {
    let inputs = ["**unclosed", "[broken](", "`no end", "![", "<div", "| a | b |", "> > >", "\u{0}"];
    for input in inputs {
        let script = markdown_to_spoken(input);
        // Best effort: whatever comes out must not contain NUL or leave empty segments.
        for seg in &script.segments {
            assert!(!seg.text.is_empty(), "no empty segment for {input:?}");
            assert_eq!(seg.text, normalize_whitespace(&seg.text), "normalized for {input:?}");
        }
    }
    let script = markdown_to_spoken("**unclosed");
    assert!(script.spoken_text().contains("unclosed"));
}

// [Requirement 4: every segment normalized | spec-narrate-text-pipeline | FR-002]
#[test]
fn every_segment_text_is_whitespace_normalized_and_non_empty() {
    let input = "#   Spaced   heading  \n\nPara   with\ttabs   and\n  continuation   lines.\n\n- item   one\n";
    let script = markdown_to_spoken(input);
    assert!(!script.is_empty());
    for seg in &script.segments {
        assert!(!seg.text.is_empty());
        assert_eq!(seg.text, normalize_whitespace(&seg.text));
    }
    assert_eq!(script.segments[0].text, "Spaced heading");
}

// [Non-ASCII text | spec-narrate-text-pipeline | FR-002 | FR-001]
#[test]
fn non_ascii_markdown_text_is_preserved() {
    assert_eq!(
        md("# 日本語\n\nこんにちは。Café **naïve**."),
        expect(&[("日本語", Heading), ("こんにちは。Café naïve.", Paragraph)])
    );
}

// ---------------------------------------------------------------------------------------
// to_spoken, SpokenScript helpers, normalize_whitespace
// ---------------------------------------------------------------------------------------

// [Public API: to_spoken | spec-narrate-text-pipeline | FR-002 | FR-003]
#[test]
fn to_spoken_dispatches_on_script_kind() {
    let text = "# Head\n\nSome **bold** text.";
    assert_eq!(
        pairs(&to_spoken(text, ScriptKind::Markdown)),
        expect(&[("Head", Heading), ("Some bold text.", Paragraph)])
    );
    // Plain text keeps Markdown syntax verbatim (FR-003: `.txt` is used as-is).
    assert_eq!(
        pairs(&to_spoken(text, ScriptKind::PlainText)),
        expect(&[("# Head", Paragraph), ("Some **bold** text.", Paragraph)])
    );
}

// [Public API: SpokenScript | spec-narrate-text-pipeline | FR-002]
#[test]
fn spoken_text_joins_segment_texts_with_a_blank_line() {
    let script = SpokenScript {
        segments: vec![
            Segment { text: "One.".to_string(), pause_after: Paragraph },
            Segment { text: "Two.".to_string(), pause_after: Heading },
        ],
    };
    assert_eq!(script.spoken_text(), "One.\n\nTwo.");
    assert!(!script.is_empty());
    assert_eq!(SpokenScript::default().spoken_text(), "");
    assert!(SpokenScript::default().is_empty());
}

// [Public API: normalize_whitespace | spec-narrate-text-pipeline | FR-002]
#[test]
fn normalize_whitespace_collapses_runs_and_trims() {
    assert_eq!(normalize_whitespace("  a \t b\n\nc  "), "a b c");
    assert_eq!(normalize_whitespace(""), "");
    assert_eq!(normalize_whitespace(" \n\t "), "");
    assert_eq!(normalize_whitespace("a\u{00A0}\u{2003}b"), "a b");
    assert_eq!(normalize_whitespace("single"), "single");
}

// [Public API: derives | spec-narrate-text-pipeline | FR-005]
#[test]
fn pause_and_segment_derive_the_documented_traits() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(Pause::Chunk);
    set.insert(Pause::Cell);
    set.insert(Pause::Paragraph);
    set.insert(Pause::Heading);
    set.insert(Pause::Heading);
    assert_eq!(set.len(), 4);
    let a = Segment { text: "x".to_string(), pause_after: Cell };
    let b = a.clone();
    assert_eq!(a, b);
}

// [Public API re-exports | spec-narrate-text-pipeline | FR-002]
#[test]
fn spoken_items_are_re_exported_at_the_crate_root() {
    let s: narrate_core::SpokenScript = narrate_core::markdown_to_spoken("Hi.");
    assert_eq!(s.segments.len(), 1);
    let _ = narrate_core::plain_to_spoken("Hi.");
    let _ = narrate_core::normalize_whitespace(" x ");
    let _ = narrate_core::to_spoken("Hi.", narrate_core::ScriptKind::PlainText);
    let _ = narrate_core::Pause::Chunk;
}
