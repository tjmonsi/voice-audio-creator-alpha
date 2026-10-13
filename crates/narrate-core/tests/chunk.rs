//! Black-box tests for sentence splitting, chunk packing and pause assignment (FR-005).
//!
//! Source: specs/spec-narrate-text-pipeline.md (Requirements 6 and 7, RULE-040 to RULE-055,
//! RULE-060). No implementation was read.

use narrate_core::chunk::{chunk_script, split_sentences, Chunk, MAX_CHUNK_CHARS};
use narrate_core::spoken::{markdown_to_spoken, normalize_whitespace, Pause, Segment, SpokenScript};

fn script(items: &[(&str, Pause)]) -> SpokenScript {
    SpokenScript {
        segments: items
            .iter()
            .map(|(t, p)| Segment { text: (*t).to_string(), pause_after: *p })
            .collect(),
    }
}

fn len(s: &str) -> usize {
    s.chars().count()
}

fn texts(chunks: &[Chunk]) -> Vec<&str> {
    chunks.iter().map(|c| c.text.as_str()).collect()
}

fn s(items: &[&str]) -> Vec<String> {
    items.iter().map(|x| (*x).to_string()).collect()
}

// ---------------------------------------------------------------------------------------
// Constants and pause table
// ---------------------------------------------------------------------------------------

// [RULE-060 | spec-narrate-text-pipeline | FR-005 | FR-002]
#[test]
fn pause_samples_are_exact_at_24_khz() {
    assert_eq!(Pause::Chunk.samples(), 4800);
    assert_eq!(Pause::Cell.samples(), 7200);
    assert_eq!(Pause::Paragraph.samples(), 12000);
    assert_eq!(Pause::Heading.samples(), 19200);
}

// [Public API: MAX_CHUNK_CHARS | spec-narrate-text-pipeline | FR-005]
#[test]
fn max_chunk_chars_is_three_hundred() {
    assert_eq!(MAX_CHUNK_CHARS, 300);
    assert_eq!(narrate_core::MAX_CHUNK_CHARS, 300);
}

// ---------------------------------------------------------------------------------------
// Requirement 6: split_sentences
// ---------------------------------------------------------------------------------------

// [Scenario: Terminators, closers, abbreviations, decimals | spec-narrate-text-pipeline | FR-005 | RULE-040 | RULE-041 | RULE-042]
#[test]
fn split_sentences_spec_example() {
    assert_eq!(
        split_sentences("Dr. Smith paid 3.50 dollars. \"Stop!\" she said... Really? Yes."),
        s(&["Dr. Smith paid 3.50 dollars.", "\"Stop!\"", "she said...", "Really?", "Yes."])
    );
}

// [Scenario: No terminator | spec-narrate-text-pipeline | FR-005]
#[test]
fn split_sentences_without_terminator_returns_the_whole_text() {
    assert_eq!(split_sentences("no punctuation at all"), s(&["no punctuation at all"]));
}

// [Requirement 6: non-empty sentences | spec-narrate-text-pipeline | FR-005]
#[test]
fn split_sentences_of_empty_or_blank_text_is_empty() {
    assert!(split_sentences("").is_empty());
    assert!(split_sentences("  \n\t ").is_empty());
}

// [RULE-042 | spec-narrate-text-pipeline | FR-005]
#[test]
fn split_sentences_does_not_split_after_listed_abbreviations() {
    for abbr in ["Mr.", "Mrs.", "Ms.", "Dr.", "Prof.", "St.", "Jr.", "Sr.", "vs.", "e.g.", "i.e."] {
        let text = format!("Ask {abbr} Jones now.");
        assert_eq!(split_sentences(&text), vec![text.clone()], "abbreviation {abbr}");
    }
}

// [RULE-042 | spec-narrate-text-pipeline | FR-005]
#[test]
fn split_sentences_abbreviation_rule_is_case_sensitive_and_single_dot_only() {
    // `dr.` (lower case) is not in the list, so it is a boundary.
    assert_eq!(split_sentences("Call dr. smith left."), s(&["Call dr.", "smith left."]));
    // A run of three dots is not a single `.`, so the exception does not apply.
    assert_eq!(split_sentences("He said Dr... what."), s(&["He said Dr...", "what."]));
}

// [RULE-041 | spec-narrate-text-pipeline | FR-005]
#[test]
fn split_sentences_requires_whitespace_or_end_after_the_terminator_run() {
    assert_eq!(split_sentences("Pay 3.50 now."), s(&["Pay 3.50 now."]));
    assert_eq!(split_sentences("Version 1.2.3 is out."), s(&["Version 1.2.3 is out."]));
    assert_eq!(split_sentences("Open file.txt please."), s(&["Open file.txt please."]));
    // End of text counts as a boundary even without whitespace.
    assert_eq!(split_sentences("Done."), s(&["Done."]));
}

// [RULE-040 | spec-narrate-text-pipeline | FR-005]
#[test]
fn split_sentences_extends_terminator_runs_over_closers() {
    assert_eq!(split_sentences("He said \"Go.\" Then left."), s(&["He said \"Go.\"", "Then left."]));
    assert_eq!(split_sentences("(See above.) Next."), s(&["(See above.)", "Next."]));
    assert_eq!(split_sentences("'Run!' Next."), s(&["'Run!'", "Next."]));
    assert_eq!(split_sentences("Is it [true?] Maybe."), s(&["Is it [true?]", "Maybe."]));
    assert_eq!(split_sentences("\u{201C}Hi.\u{201D} Then."), s(&["\u{201C}Hi.\u{201D}", "Then."]));
    assert_eq!(split_sentences("It\u{2019}s done.\u{2019} Next."), s(&["It\u{2019}s done.\u{2019}", "Next."]));
    assert_eq!(split_sentences("\u{00AB}Oui.\u{00BB} Non."), s(&["\u{00AB}Oui.\u{00BB}", "Non."]));
}

// [RULE-040 | spec-narrate-text-pipeline | FR-005]
#[test]
fn split_sentences_handles_mixed_terminators_and_ellipsis_character() {
    assert_eq!(split_sentences("Really?! Yes."), s(&["Really?!", "Yes."]));
    assert_eq!(split_sentences("Wait\u{2026} what"), s(&["Wait\u{2026}", "what"]));
    assert_eq!(split_sentences("Hmm... okay. Fine!"), s(&["Hmm...", "okay.", "Fine!"]));
}

// [RULE-043 | spec-narrate-text-pipeline | FR-005]
#[test]
fn split_sentences_treats_cjk_terminators_as_boundaries_regardless_of_following_text() {
    assert_eq!(split_sentences("\u{4F60}\u{597D}\u{3002}\u{518D}\u{89C1}"), s(&["\u{4F60}\u{597D}\u{3002}", "\u{518D}\u{89C1}"]));
    assert_eq!(split_sentences("\u{597D}\u{FF01}\u{4E0D}"), s(&["\u{597D}\u{FF01}", "\u{4E0D}"]));
    assert_eq!(split_sentences("\u{5417}\u{FF1F}\u{662F}"), s(&["\u{5417}\u{FF1F}", "\u{662F}"]));
    // Following closers stay attached.
    assert_eq!(
        split_sentences("\u{4F60}\u{597D}\u{3002}\u{201D}\u{518D}\u{89C1}"),
        s(&["\u{4F60}\u{597D}\u{3002}\u{201D}", "\u{518D}\u{89C1}"])
    );
}

// [Requirement 6: whitespace normalization | spec-narrate-text-pipeline | FR-005]
#[test]
fn split_sentences_normalizes_whitespace_between_and_inside_sentences() {
    assert_eq!(split_sentences("One.  Two.\nThree.\t\tFour  words here."), s(&["One.", "Two.", "Three.", "Four words here."]));
}

// [Requirement 6: join property | spec-narrate-text-pipeline | FR-005]
#[test]
fn split_sentences_join_equals_normalized_text_when_boundaries_are_followed_by_whitespace() {
    let text = "First one.  Second one!\nThird one? Fourth\tone... Fifth, with a comma. Dr. Who arrives.";
    let joined = split_sentences(text).join(" ");
    assert_eq!(joined, normalize_whitespace(text));
}

// [Requirement 6: Unicode safety | spec-narrate-text-pipeline | FR-005]
#[test]
fn split_sentences_does_not_panic_on_emoji_and_combining_marks() {
    let text = "Hello \u{1F600}. e\u{0301}t\u{00E9}! \u{1F468}\u{200D}\u{1F469} done.";
    let parts = split_sentences(text);
    assert_eq!(parts.join(" "), normalize_whitespace(text));
}

// ---------------------------------------------------------------------------------------
// Requirement 7: chunk_script
// ---------------------------------------------------------------------------------------

fn prose_paragraph(seed: usize, sentences: usize) -> String {
    let words = [
        "amber", "river", "quiet", "lantern", "morning", "bridge", "violet", "harvest", "signal",
        "meadow", "copper", "window",
    ];
    let mut out = Vec::new();
    for sent_no in 0..sentences {
        let count = 8 + (seed + sent_no) % 7;
        let mut sentence = String::new();
        for w in 0..count {
            if w > 0 {
                sentence.push(' ');
            }
            sentence.push_str(words[(seed * 7 + sent_no * 3 + w) % words.len()]);
        }
        sentence.push('.');
        out.push(sentence);
    }
    out.join(" ")
}

// [Scenario: Completeness and order | spec-narrate-text-pipeline | FR-005 | UR-001]
#[test]
fn chunks_cover_the_text_in_order_and_end_on_sentence_ends() {
    let p1 = prose_paragraph(1, 9);
    let p2 = prose_paragraph(2, 9);
    let p3 = prose_paragraph(3, 9);
    let sc = script(&[(&p1, Pause::Paragraph), (&p2, Pause::Paragraph), (&p3, Pause::Paragraph)]);
    assert!(len(&sc.spoken_text()) >= 1500, "fixture is long prose");

    let chunks = chunk_script(&sc, 300);
    assert!(chunks.len() >= 5);
    for c in &chunks {
        assert!(len(&c.text) <= 300, "chunk {} has {} chars", c.number, len(&c.text));
    }
    let joined = texts(&chunks).join(" ");
    assert_eq!(normalize_whitespace(&joined), normalize_whitespace(&sc.spoken_text()));

    let all_sentences: Vec<String> = sc
        .segments
        .iter()
        .flat_map(|seg| split_sentences(&seg.text))
        .collect();
    for c in &chunks {
        assert!(
            all_sentences.iter().any(|sent| c.text.ends_with(sent.as_str())),
            "chunk {} must end with the end of a sentence: {:?}",
            c.number,
            c.text
        );
    }
    // 1-based consecutive numbering.
    for (i, c) in chunks.iter().enumerate() {
        assert_eq!(c.number, i + 1);
    }
}

// [Scenario: Over-long sentence | spec-narrate-text-pipeline | FR-005 | RULE-052]
fn long_sentence_with_comma_at_250() -> String {
    // 700 characters; single spaces at indices where i % 10 == 1; comma at index 250 followed by
    // the space at index 251; final character is the only terminator.
    let mut chars: Vec<char> = (0..700)
        .map(|i| if i % 10 == 1 { ' ' } else { (b'a' + (i % 7) as u8) as char })
        .collect();
    chars[250] = ',';
    chars[699] = '.';
    chars.into_iter().collect()
}

// [Scenario: Over-long sentence | spec-narrate-text-pipeline | FR-005 | RULE-052]
#[test]
fn over_long_sentence_is_cut_after_the_comma_and_never_exceeds_the_limit() {
    let sentence = long_sentence_with_comma_at_250();
    assert_eq!(len(&sentence), 700);
    let chunks = chunk_script(&script(&[(&sentence, Pause::Paragraph)]), 300);
    assert_eq!(chunks.len(), 3);
    assert!(chunks[0].text.ends_with(','), "first chunk ends with the comma: {:?}", chunks[0].text);
    assert_eq!(len(&chunks[0].text), 251);
    for c in &chunks {
        assert!(len(&c.text) <= 300);
        assert_eq!(c.text, c.text.trim());
    }
    assert_eq!(
        normalize_whitespace(&texts(&chunks).join(" ")),
        normalize_whitespace(&sentence)
    );
    // Only the last chunk of the segment carries the segment pause.
    assert_eq!(chunks[0].pause_after, Pause::Chunk);
    assert_eq!(chunks[1].pause_after, Pause::Chunk);
    assert_eq!(chunks[2].pause_after, Pause::Paragraph);
}

// [Scenario: Over-long sentence | spec-narrate-text-pipeline | FR-005 | RULE-052]
#[test]
fn only_the_over_long_sentence_is_split_inside_itself() {
    let long = long_sentence_with_comma_at_250();
    let segment_text = format!("Short lead. {long} Short tail.");
    let chunks = chunk_script(&script(&[(&segment_text, Pause::Paragraph)]), 300);
    assert_eq!(chunks.len(), 5);
    assert_eq!(chunks[0].text, "Short lead.");
    assert!(chunks[1].text.ends_with(','));
    assert_eq!(chunks[4].text, "Short tail.");
    for c in &chunks {
        assert!(len(&c.text) <= 300);
    }
}

// [Scenario: Pause assignment | spec-narrate-text-pipeline | FR-005 | FR-002 | RULE-055]
#[test]
fn pause_assignment_matches_the_spec_example() {
    let sentences: Vec<String> = (1..=7)
        .map(|i| {
            let head = format!("S{i} ");
            format!("{head}{}.", "b".repeat(99 - head.len() - 1))
        })
        .collect();
    assert!(sentences.iter().all(|x| len(x) == 99));
    let paragraph = sentences.join(" ");
    let chunks = chunk_script(
        &script(&[("Title", Pause::Heading), (&paragraph, Pause::Paragraph)]),
        300,
    );
    assert_eq!(chunks.len(), 4);
    assert_eq!(chunks[0].text, "Title");
    assert_eq!(chunks[0].pause_after, Pause::Heading);
    assert_eq!(chunks[1].text, sentences[0..3].join(" "));
    assert_eq!(len(&chunks[1].text), 299);
    assert_eq!(chunks[1].pause_after, Pause::Chunk);
    assert_eq!(chunks[2].text, sentences[3..6].join(" "));
    assert_eq!(len(&chunks[2].text), 299);
    assert_eq!(chunks[2].pause_after, Pause::Chunk);
    assert_eq!(chunks[3].text, sentences[6]);
    assert_eq!(chunks[3].pause_after, Pause::Paragraph);
    for (i, c) in chunks.iter().enumerate() {
        assert_eq!(c.number, i + 1);
    }
}

fn sentence_with_tag_inside_window() -> String {
    // 400 characters, no commas. Index 288 is a space, `[clear throat]` occupies 289..=302,
    // index 303 is a space, and 289..=299 hold no whitespace except the tag's own space at 295.
    let tag: Vec<char> = "[clear throat]".chars().collect();
    let mut chars: Vec<char> = Vec::with_capacity(400);
    for i in 0..400usize {
        let c = if (289..303).contains(&i) {
            tag[i - 289]
        } else if i == 288 || i == 303 {
            ' '
        } else if i < 288 && i % 10 == 8 {
            ' '
        } else if i > 303 && i % 10 == 8 {
            ' '
        } else {
            (b'a' + (i % 5) as u8) as char
        };
        chars.push(c);
    }
    chars[399] = '.';
    chars.into_iter().collect()
}

// [Scenario: Tag not split | spec-narrate-text-pipeline | FR-005 | FR-003 | RULE-053]
#[test]
fn a_tag_crossing_the_window_is_not_split() {
    let sentence = sentence_with_tag_inside_window();
    let chars: Vec<char> = sentence.chars().collect();
    assert_eq!(chars.len(), 400);
    assert_eq!(chars[288], ' ');
    assert_eq!(chars[289..303].iter().collect::<String>(), "[clear throat]");
    assert_eq!(chars[295], ' ');
    assert!(!sentence.contains(','));
    let chunks = chunk_script(&script(&[(&sentence, Pause::Paragraph)]), 300);
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].text, chars[0..288].iter().collect::<String>());
    assert!(chunks[1].text.starts_with("[clear throat]"));
}

// [RULE-052, RULE-053 | spec-narrate-text-pipeline | FR-005 | FR-003]
#[test]
fn hard_cut_inside_a_tag_moves_back_to_the_start_of_the_tag() {
    // 290 letters, then the tag, then 96 letters: no other whitespace, no comma, so the cut
    // falls at character 300, inside the tag span [290, 304), and moves back to index 290.
    let head: String = "a".repeat(290);
    let tail: String = "b".repeat(95) + ".";
    let sentence = format!("{head}[clear throat]{tail}");
    assert_eq!(len(&sentence), 400);
    let chunks = chunk_script(&script(&[(&sentence, Pause::Paragraph)]), 300);
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].text, head);
    assert!(chunks[1].text.starts_with("[clear throat]"));
    assert!(len(&chunks[0].text) <= 300 && len(&chunks[1].text) <= 300);
}

// [RULE-052 | spec-narrate-text-pipeline | FR-005]
#[test]
fn unbreakable_text_is_cut_exactly_at_the_limit() {
    let word = "x".repeat(650);
    let chunks = chunk_script(&script(&[(&word, Pause::Paragraph)]), 300);
    assert_eq!(chunks.len(), 3);
    assert_eq!(len(&chunks[0].text), 300);
    assert_eq!(len(&chunks[1].text), 300);
    assert_eq!(len(&chunks[2].text), 50);
}

// [RULE-054 | spec-narrate-text-pipeline | FR-005 | FR-002]
#[test]
fn chunks_never_span_segments_and_each_segment_keeps_its_own_pause() {
    let chunks = chunk_script(
        &script(&[("Hello.", Pause::Paragraph), ("World.", Pause::Paragraph), ("Name", Pause::Cell)]),
        300,
    );
    assert_eq!(texts(&chunks), vec!["Hello.", "World.", "Name"]);
    assert_eq!(
        chunks.iter().map(|c| c.pause_after).collect::<Vec<_>>(),
        vec![Pause::Paragraph, Pause::Paragraph, Pause::Cell]
    );
}

// [RULE-055 | spec-narrate-text-pipeline | FR-005]
#[test]
fn the_last_chunk_of_the_script_keeps_its_segment_pause() {
    let one = chunk_script(&script(&[("Only one.", Pause::Paragraph)]), 300);
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].pause_after, Pause::Paragraph);
    assert_eq!(one[0].number, 1);
    let heading = chunk_script(&script(&[("Title", Pause::Heading)]), 300);
    assert_eq!(heading[0].pause_after, Pause::Heading);
}

// [Public API: chunk_script | spec-narrate-text-pipeline | FR-005]
#[test]
fn empty_script_gives_no_chunks() {
    assert!(chunk_script(&SpokenScript::default(), 300).is_empty());
}

// [RULE-051 | spec-narrate-text-pipeline | FR-005]
#[test]
fn packing_boundary_exactly_at_the_limit_stays_in_one_chunk() {
    let a = format!("{}.", "a".repeat(148)); // 149 chars
    let b150 = format!("{}.", "b".repeat(149)); // 150 chars
    let b151 = format!("{}.", "b".repeat(150)); // 151 chars
    let fits = format!("{a} {b150}"); // 149 + 1 + 150 = 300
    let over = format!("{a} {b151}"); // 301
    let one = chunk_script(&script(&[(&fits, Pause::Paragraph)]), 300);
    assert_eq!(one.len(), 1);
    assert_eq!(len(&one[0].text), 300);
    let two = chunk_script(&script(&[(&over, Pause::Paragraph)]), 300);
    assert_eq!(two.len(), 2);
    assert_eq!(two[0].text, a);
    assert_eq!(two[1].text, b151);
    assert_eq!(two[0].pause_after, Pause::Chunk);
    assert_eq!(two[1].pause_after, Pause::Paragraph);
}

// [RULE-052 | spec-narrate-text-pipeline | FR-005]
#[test]
fn single_sentence_of_exactly_the_limit_is_one_chunk_and_one_more_character_splits_it() {
    let make = |n: usize| -> String {
        // Words of 9 letters separated by single spaces, ending with a period, n chars total.
        let mut out = String::new();
        while out.chars().count() < n {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str("abcdefghi");
        }
        let mut chars: Vec<char> = out.chars().take(n).collect();
        chars[n - 1] = '.';
        if n >= 2 && chars[n - 2] == ' ' {
            chars[n - 2] = 'z';
        }
        chars.into_iter().collect()
    };
    let exact = make(300);
    assert_eq!(len(&exact), 300);
    assert_eq!(chunk_script(&script(&[(&exact, Pause::Paragraph)]), 300).len(), 1);
    let over = make(301);
    assert_eq!(len(&over), 301);
    let chunks = chunk_script(&script(&[(&over, Pause::Paragraph)]), 300);
    assert_eq!(chunks.len(), 2);
    assert!(chunks.iter().all(|c| len(&c.text) <= 300));
}

// [RULE-050 | spec-narrate-text-pipeline | FR-005]
#[test]
fn length_counts_unicode_scalar_values_not_bytes() {
    // 250 two-byte characters plus a period: 251 chars, 501 bytes. One chunk when counting
    // characters; two or more if an implementation counted bytes.
    let sentence = format!("{}.", "\u{00E9}".repeat(250));
    assert_eq!(len(&sentence), 251);
    let chunks = chunk_script(&script(&[(&sentence, Pause::Paragraph)]), 300);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].text, sentence);
}

// [Public API: chunk_script with other limits | spec-narrate-text-pipeline | FR-005]
#[test]
fn smaller_limits_are_honored_and_content_is_preserved() {
    let words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
    let text = format!("{words}. {words}! {words}?");
    for max in [10usize, 25, 50, 120, 300] {
        let chunks = chunk_script(&script(&[(&text, Pause::Paragraph)]), max);
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert!(len(&c.text) <= max, "max {max}: chunk {:?} is {} chars", c.text, len(&c.text));
        }
        assert_eq!(
            normalize_whitespace(&texts(&chunks).join(" ")),
            normalize_whitespace(&text),
            "max {max}"
        );
    }
}

// [Scenario: Pause assignment, Markdown input | spec-narrate-text-pipeline | FR-002 | FR-005]
#[test]
fn markdown_heading_then_paragraph_chunks_carry_heading_and_paragraph_pauses() {
    let sc = markdown_to_spoken("# Title\n\nBody one. Body two.");
    let chunks = chunk_script(&sc, 300);
    assert_eq!(texts(&chunks), vec!["Title", "Body one. Body two."]);
    assert_eq!(chunks[0].pause_after, Pause::Heading);
    assert_eq!(chunks[1].pause_after, Pause::Paragraph);
}

// [Scenario: Pause assignment, tables | spec-narrate-text-pipeline | FR-002 | RULE-021 | RULE-055]
#[test]
fn table_cells_become_separate_chunks_with_cell_pauses() {
    let sc = markdown_to_spoken("| Name | Age |\n|------|-----|\n| Ann  | 30  |\n");
    let chunks = chunk_script(&sc, 300);
    assert_eq!(texts(&chunks), vec!["Name", "Age", "Ann", "30"]);
    assert_eq!(
        chunks.iter().map(|c| c.pause_after).collect::<Vec<_>>(),
        vec![Pause::Cell, Pause::Paragraph, Pause::Cell, Pause::Paragraph]
    );
}

// [Output: total silence | spec-narrate-text-pipeline | FR-005 | RULE-060]
#[test]
fn total_silence_is_the_sum_of_pause_samples_over_all_chunks() {
    let sc = script(&[("Title", Pause::Heading), ("One. Two.", Pause::Paragraph), ("Cell", Pause::Cell)]);
    let chunks = chunk_script(&sc, 300);
    let total: u32 = chunks.iter().map(|c| c.pause_after.samples()).sum();
    assert_eq!(total, 19200 + 12000 + 7200);
}

// [Public API re-exports | spec-narrate-text-pipeline | FR-005]
#[test]
fn chunk_items_are_re_exported_at_the_crate_root() {
    let chunks: Vec<narrate_core::Chunk> =
        narrate_core::chunk_script(&narrate_core::plain_to_spoken("Hello. World."), 300);
    assert_eq!(chunks.len(), 1);
    assert_eq!(narrate_core::split_sentences("A. B."), s(&["A.", "B."]));
}
