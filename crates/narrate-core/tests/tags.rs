//! Black-box tests for paralinguistic tag handling: `PARALINGUISTIC_TAGS`, `is_known_tag`,
//! `unknown_tags` (RULE-030) and verbatim pass-through through both converters
//! (Requirement 5, RULE-024, RULE-053).
//!
//! Source: specs/spec-narrate-text-pipeline.md. No implementation was read.

use narrate_core::spoken::{markdown_to_spoken, plain_to_spoken, SpokenScript};
use narrate_core::tags::{is_known_tag, unknown_tags, PARALINGUISTIC_TAGS};

fn only_text(script: &SpokenScript) -> String {
    assert_eq!(script.segments.len(), 1, "expected exactly one segment: {script:?}");
    script.segments[0].text.clone()
}

const EXPECTED_TAGS: [&str; 19] = [
    "[angry]",
    "[fear]",
    "[surprised]",
    "[whispering]",
    "[advertisement]",
    "[dramatic]",
    "[narration]",
    "[crying]",
    "[happy]",
    "[sarcastic]",
    "[clear throat]",
    "[sigh]",
    "[shush]",
    "[cough]",
    "[groan]",
    "[sniff]",
    "[gasp]",
    "[chuckle]",
    "[laugh]",
];

// ---------------------------------------------------------------------------------------
// PARALINGUISTIC_TAGS and is_known_tag
// ---------------------------------------------------------------------------------------

// [Public API: PARALINGUISTIC_TAGS | spec-narrate-text-pipeline | FR-003]
#[test]
fn paralinguistic_tags_are_the_nineteen_bracketed_tags_in_order() {
    assert_eq!(PARALINGUISTIC_TAGS.len(), 19);
    assert_eq!(PARALINGUISTIC_TAGS, EXPECTED_TAGS);
}

// [Public API: is_known_tag | spec-narrate-text-pipeline | FR-003]
#[test]
fn is_known_tag_accepts_every_listed_tag() {
    for tag in EXPECTED_TAGS {
        assert!(is_known_tag(tag), "{tag} must be known");
    }
}

// [Public API: is_known_tag | spec-narrate-text-pipeline | FR-003]
#[test]
fn is_known_tag_is_exact_and_case_sensitive() {
    // Invalid / near-miss / null-like inputs.
    for tag in [
        "", "laugh", "[Laugh]", "[LAUGH]", "[laugh", "laugh]", "[ laugh]", "[laugh ]",
        "[clear  throat]", "[clear throat ]", "[dance]", "[laugh][laugh]", " [laugh]",
    ] {
        assert!(!is_known_tag(tag), "{tag:?} must not be known");
    }
}

// [Public API re-exports | spec-narrate-text-pipeline | FR-003]
#[test]
fn tag_items_are_re_exported_at_the_crate_root() {
    assert_eq!(narrate_core::PARALINGUISTIC_TAGS, EXPECTED_TAGS);
    assert!(narrate_core::is_known_tag("[sigh]"));
    assert_eq!(narrate_core::unknown_tags("[dance]"), vec!["[dance]".to_string()]);
}

// ---------------------------------------------------------------------------------------
// unknown_tags (RULE-030)
// ---------------------------------------------------------------------------------------

// [Scenario: Unknown and malformed tags | spec-narrate-text-pipeline | FR-003 | RULE-030]
#[test]
fn unknown_tags_reports_only_the_unknown_well_formed_tag() {
    let text = "Try [dance] and [laugh then [clear throat] go.";
    assert_eq!(unknown_tags(text), vec!["[dance]".to_string()]);
}

// [Scenario: Unknown tag warning | spec-narrate-generation Req 6 | FR-003 | RULE-030]
#[test]
fn unknown_tags_has_no_duplicates_and_ignores_known_tags() {
    assert_eq!(
        unknown_tags("Hi [dance] there [dance] [laugh]."),
        vec!["[dance]".to_string()]
    );
}

// [RULE-030 | spec-narrate-text-pipeline | FR-003]
#[test]
fn unknown_tags_keeps_first_appearance_order() {
    assert_eq!(
        unknown_tags("[zeta] [alpha] [zeta] [beta] [alpha]"),
        vec!["[zeta]".to_string(), "[alpha]".to_string(), "[beta]".to_string()]
    );
}

// [RULE-030 | spec-narrate-text-pipeline | FR-003]
#[test]
fn unknown_tags_accepts_one_or_two_lowercase_words_separated_by_one_space() {
    assert_eq!(unknown_tags("[big sigh]"), vec!["[big sigh]".to_string()]);
    assert_eq!(unknown_tags("[a]"), vec!["[a]".to_string()]);
    // A known two-word tag is not reported.
    assert!(unknown_tags("[clear throat]").is_empty());
}

// [RULE-030 | spec-narrate-text-pipeline | FR-003]
#[test]
fn unknown_tags_ignores_things_that_do_not_match_the_pattern() {
    // Boundary / invalid: digits, uppercase, unclosed, empty, padded, three words, two spaces,
    // hyphen, non-ASCII letters, punctuation inside.
    for text in [
        "[1]", "[Dance]", "[DANCE]", "[laugh", "[]", "[ dance]", "[dance ]", "[a b c]",
        "[big  sigh]", "[well-known]", "[dänce]", "[dance2]", "[dance!]", "[big_sigh]", "",
        "no brackets at all",
    ] {
        assert!(unknown_tags(text).is_empty(), "{text:?} must report nothing");
    }
}

// [RULE-030 | spec-narrate-text-pipeline | FR-003]
#[test]
fn unknown_tags_finds_adjacent_and_embedded_tags() {
    assert_eq!(
        unknown_tags("[dance][sing]"),
        vec!["[dance]".to_string(), "[sing]".to_string()]
    );
    assert!(unknown_tags("[laugh][laugh]").is_empty());
    assert_eq!(unknown_tags("word[dance]word"), vec!["[dance]".to_string()]);
}

// [RULE-030 | spec-narrate-text-pipeline | FR-003]
#[test]
fn unknown_tags_is_silent_for_all_nineteen_known_tags() {
    let text = EXPECTED_TAGS.join(" ");
    assert!(unknown_tags(&text).is_empty());
}

// ---------------------------------------------------------------------------------------
// Requirement 5: pass-through
// ---------------------------------------------------------------------------------------

// [Scenario: Known tag kept | spec-narrate-text-pipeline | FR-003 | RULE-024]
#[test]
fn markdown_keeps_known_tags_verbatim() {
    let script = markdown_to_spoken("Well [chuckle] that worked. [laugh]");
    assert_eq!(only_text(&script), "Well [chuckle] that worked. [laugh]");
}

// [Scenario: Tag followed by a link target is a link | spec-narrate-text-pipeline | FR-003 | RULE-014]
#[test]
fn markdown_treats_tag_with_destination_as_a_link() {
    let script = markdown_to_spoken("Click [laugh](https://x.y) now.");
    assert_eq!(only_text(&script), "Click laugh now.");
}

// [Scenario: Unknown and malformed tags | spec-narrate-text-pipeline | FR-003 | RULE-024]
#[test]
fn markdown_keeps_unknown_and_unclosed_tags_verbatim_and_unknown_tags_flags_only_the_unknown() {
    let text = "Try [dance] and [laugh then [clear throat] go.";
    let script = markdown_to_spoken(text);
    let segment_text = only_text(&script);
    assert_eq!(segment_text, text);
    assert_eq!(unknown_tags(&segment_text), vec!["[dance]".to_string()]);
}

// [Scenario: Known tag kept | spec-narrate-text-pipeline | FR-003 | RULE-024]
#[test]
fn markdown_keeps_each_of_the_nineteen_tags_verbatim() {
    for tag in EXPECTED_TAGS {
        let text = format!("Before {tag} after.");
        assert_eq!(only_text(&markdown_to_spoken(&text)), text, "tag {tag}");
    }
}

// [Scenario: Known tag kept | spec-narrate-text-pipeline | FR-003 | RULE-024]
#[test]
fn markdown_keeps_a_tag_at_the_start_and_end_of_a_paragraph() {
    assert_eq!(only_text(&markdown_to_spoken("[sigh] ok")), "[sigh] ok");
    assert_eq!(only_text(&markdown_to_spoken("ok [sigh]")), "ok [sigh]");
    assert_eq!(only_text(&markdown_to_spoken("[sigh]")), "[sigh]");
}

// [Scenario: Paragraphs and pass-through | spec-narrate-text-pipeline | FR-003]
#[test]
fn plain_text_keeps_tags_markup_and_unclosed_brackets_unchanged() {
    assert_eq!(
        only_text(&plain_to_spoken("Well [chuckle] that **worked**. [dance] and [laugh")),
        "Well [chuckle] that **worked**. [dance] and [laugh"
    );
    assert_eq!(
        only_text(&plain_to_spoken("Click [laugh](https://x.y) now.")),
        "Click [laugh](https://x.y) now."
    );
}

// [RULE-053 | spec-narrate-text-pipeline | FR-003 | FR-005]
#[test]
fn multi_word_tag_survives_both_converters_with_its_single_space() {
    assert_eq!(only_text(&markdown_to_spoken("Ahem [clear throat] hello.")), "Ahem [clear throat] hello.");
    assert_eq!(only_text(&plain_to_spoken("Ahem [clear throat] hello.")), "Ahem [clear throat] hello.");
}
