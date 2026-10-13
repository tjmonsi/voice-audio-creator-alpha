//! Sentence splitting and chunk packing (spec-narrate-text-pipeline.md, Requirements 6 and 7;
//! RULE-040 to RULE-055).

use crate::spoken::{Pause, SpokenScript, normalize_whitespace};
use crate::tags::PARALINGUISTIC_TAGS;

/// Longest chunk, in characters, that generation sends to the model (research P1 section 1.6).
pub const MAX_CHUNK_CHARS: usize = 300;

/// A model-sized piece of speech and the pause after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// 1-based position in the script.
    pub number: usize,
    /// Text for one synthesis call, at most `max_chars` characters.
    pub text: String,
    /// Silence after the chunk.
    pub pause_after: Pause,
}

const ABBREVIATIONS: [&str; 11] = [
    "Mr.", "Mrs.", "Ms.", "Dr.", "Prof.", "St.", "Jr.", "Sr.", "vs.", "e.g.", "i.e.",
];

fn is_terminator(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '\u{2026}')
}

fn is_closer(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | ')' | ']' | '\u{201d}' | '\u{2019}' | '\u{00bb}'
    )
}

fn is_wide_terminator(c: char) -> bool {
    matches!(c, '\u{3002}' | '\u{ff01}' | '\u{ff1f}')
}

/// Split text into sentences (RULE-040 to RULE-043). Sentences are trimmed, whitespace-normalized,
/// and non-empty.
pub fn split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = normalize_whitespace(text).chars().collect();
    let mut sentences = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_wide_terminator(c) {
            let mut end = i;
            while end < chars.len() && is_wide_terminator(chars[end]) {
                end += 1;
            }
            end = skip_closers(&chars, end);
            push_sentence(&mut sentences, &chars[start..end]);
            start = end;
            i = end;
        } else if is_terminator(c) {
            let mut run_end = i;
            while run_end < chars.len() && is_terminator(chars[run_end]) {
                run_end += 1;
            }
            let end = skip_closers(&chars, run_end);
            let at_boundary = end == chars.len() || chars[end].is_whitespace();
            let lone_period = end == run_end && run_end - i == 1 && c == '.';
            if at_boundary && !(lone_period && ends_abbreviation(&chars[..run_end])) {
                push_sentence(&mut sentences, &chars[start..end]);
                start = end;
            }
            i = end;
        } else {
            i += 1;
        }
    }
    push_sentence(&mut sentences, &chars[start..]);
    sentences
}

fn skip_closers(chars: &[char], mut at: usize) -> usize {
    while at < chars.len() && is_closer(chars[at]) {
        at += 1;
    }
    at
}

fn push_sentence(sentences: &mut Vec<String>, chars: &[char]) {
    let sentence: String = chars.iter().collect();
    let sentence = sentence.trim();
    if !sentence.is_empty() {
        sentences.push(sentence.to_owned());
    }
}

/// RULE-042: does the whitespace-delimited word that ends `chars` match a known abbreviation?
fn ends_abbreviation(chars: &[char]) -> bool {
    let word_start = chars
        .iter()
        .rposition(|c| c.is_whitespace())
        .map_or(0, |p| p + 1);
    let word: String = chars[word_start..].iter().collect();
    ABBREVIATIONS.contains(&word.as_str())
}

/// Pack the sentences of each segment into chunks of at most `max_chars` characters
/// (RULE-050 to RULE-055).
pub fn chunk_script(script: &SpokenScript, max_chars: usize) -> Vec<Chunk> {
    let max_chars = max_chars.max(1);
    let mut chunks = Vec::new();
    for segment in &script.segments {
        let first_of_segment = chunks.len();
        let mut current = String::new();
        for sentence in split_sentences(&segment.text) {
            let length = sentence.chars().count();
            if length > max_chars {
                close_chunk(&mut chunks, &mut current);
                for piece in cut_long_sentence(&sentence, max_chars) {
                    push_chunk(&mut chunks, piece);
                }
            } else if current.is_empty() {
                current = sentence;
            } else if current.chars().count() + 1 + length <= max_chars {
                current.push(' ');
                current.push_str(&sentence);
            } else {
                close_chunk(&mut chunks, &mut current);
                current = sentence;
            }
        }
        close_chunk(&mut chunks, &mut current);
        if let Some(last) = chunks[first_of_segment..].last_mut() {
            last.pause_after = segment.pause_after;
        }
    }
    chunks
}

fn close_chunk(chunks: &mut Vec<Chunk>, current: &mut String) {
    if !current.is_empty() {
        push_chunk(chunks, std::mem::take(current));
    }
}

fn push_chunk(chunks: &mut Vec<Chunk>, text: String) {
    let number = chunks.len() + 1;
    chunks.push(Chunk {
        number,
        text,
        pause_after: Pause::Chunk,
    });
}

/// RULE-052: cut one over-long sentence into pieces of at most `max_chars` characters.
fn cut_long_sentence(sentence: &str, max_chars: usize) -> Vec<String> {
    let mut rest: Vec<char> = sentence.chars().collect();
    let mut pieces = Vec::new();
    while rest.len() > max_chars {
        let cut = find_cut(&rest, max_chars);
        let piece: String = rest[..cut].iter().collect();
        pieces.push(piece.trim().to_owned());
        let remainder: String = rest[cut..].iter().collect();
        rest = remainder.trim_start().chars().collect();
    }
    let tail: String = rest.iter().collect();
    if !tail.trim().is_empty() {
        pieces.push(tail.trim().to_owned());
    }
    pieces
}

/// Character index at which to cut `rest` (longer than `max_chars`): after the last `, ; :`
/// followed by whitespace in the second and third thirds of the window, else at the last
/// whitespace outside a known tag, else at the window edge moved back to a tag start.
fn find_cut(rest: &[char], max_chars: usize) -> usize {
    let spans = tag_spans(rest);
    let in_tag = |index: usize| {
        spans
            .iter()
            .any(|&(start, end)| index >= start && index < end)
    };
    let window = &rest[..max_chars];

    let clause = (max_chars / 3..max_chars).rev().find(|&i| {
        matches!(window[i], ',' | ';' | ':') && rest.get(i + 1).is_some_and(|c| c.is_whitespace())
    });
    if let Some(i) = clause {
        return i + 1;
    }
    if let Some(i) = (1..max_chars)
        .rev()
        .find(|&i| window[i].is_whitespace() && !in_tag(i))
    {
        return i;
    }
    match spans
        .iter()
        .find(|&&(start, end)| start < max_chars && max_chars < end)
    {
        Some(&(start, _)) if start > 0 => start,
        _ => max_chars,
    }
}

/// Character ranges `[start, end)` of every known tag in `chars`.
fn tag_spans(chars: &[char]) -> Vec<(usize, usize)> {
    let tags: Vec<Vec<char>> = PARALINGUISTIC_TAGS
        .iter()
        .map(|t| t.chars().collect())
        .collect();
    let mut spans = Vec::new();
    for start in 0..chars.len() {
        if chars[start] != '[' {
            continue;
        }
        if let Some(tag) = tags.iter().find(|t| chars[start..].starts_with(t)) {
            spans.push((start, start + tag.len()));
        }
    }
    spans
}
