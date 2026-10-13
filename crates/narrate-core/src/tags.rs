//! Paralinguistic tags understood by Chatterbox-Turbo (research P1 section 1.7).

/// The 19 tags the Turbo tokenizer adds, bracketed, in token-id order (50257 to 50275).
pub const PARALINGUISTIC_TAGS: [&str; 19] = [
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

/// True when `tag` (with brackets) is one of [`PARALINGUISTIC_TAGS`]; the match is exact and
/// case-sensitive.
pub fn is_known_tag(tag: &str) -> bool {
    PARALINGUISTIC_TAGS.contains(&tag)
}

/// Every bracketed token shaped like a tag that the model does not know, in first-appearance
/// order without duplicates (RULE-030).
///
/// A token is `[` + one or two runs of ASCII lowercase letters separated by one space + `]`.
pub fn unknown_tags(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut found: Vec<String> = Vec::new();
    for (start, &c) in chars.iter().enumerate() {
        if c != '[' {
            continue;
        }
        let Some(end) = tag_end(&chars, start) else {
            continue;
        };
        let tag: String = chars[start..=end].iter().collect();
        if !is_known_tag(&tag) && !found.contains(&tag) {
            found.push(tag);
        }
    }
    found
}

/// Index of the closing `]` when a tag-shaped token starts at `start`.
fn tag_end(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start + 1;
    let first = lowercase_run(chars, i);
    if first == 0 {
        return None;
    }
    i += first;
    if chars.get(i) == Some(&' ') {
        let second = lowercase_run(chars, i + 1);
        if second == 0 {
            return None;
        }
        i += 1 + second;
    }
    (chars.get(i) == Some(&']')).then_some(i)
}

fn lowercase_run(chars: &[char], from: usize) -> usize {
    chars.get(from..).map_or(0, |rest| {
        rest.iter().take_while(|c| c.is_ascii_lowercase()).count()
    })
}
