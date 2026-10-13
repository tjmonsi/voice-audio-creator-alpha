//! Conversion of script text to speakable segments (spec-narrate-text-pipeline.md,
//! Requirements 3 to 5; RULE-010 to RULE-024).

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

use crate::script::ScriptKind;

/// Silence that follows a chunk of speech.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pause {
    /// Between chunks of one segment.
    Chunk,
    /// After a table cell that is not the last in its row.
    Cell,
    /// After a paragraph, list item, or table row.
    Paragraph,
    /// After a heading.
    Heading,
}

impl Pause {
    /// Silence length in samples at 24 kHz (RULE-060).
    pub const fn samples(self) -> u32 {
        match self {
            Pause::Chunk => 4800,
            Pause::Cell => 7200,
            Pause::Paragraph => 12000,
            Pause::Heading => 19200,
        }
    }
}

/// One block of speakable text and the pause after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Whitespace-normalized text.
    pub text: String,
    /// Silence after the segment.
    pub pause_after: Pause,
}

/// The ordered segments of a script.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpokenScript {
    /// Segments in document order.
    pub segments: Vec<Segment>,
}

impl SpokenScript {
    /// True when the script has no segments.
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Segment texts joined with a blank line.
    pub fn spoken_text(&self) -> String {
        let texts: Vec<&str> = self.segments.iter().map(|s| s.text.as_str()).collect();
        texts.join("\n\n")
    }
}

/// Convert script text according to its kind.
pub fn to_spoken(text: &str, kind: ScriptKind) -> SpokenScript {
    match kind {
        ScriptKind::Markdown => markdown_to_spoken(text),
        ScriptKind::PlainText => plain_to_spoken(text),
    }
}

/// Replace every run of whitespace with one space and trim the result.
pub fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split plain text into paragraphs at blank lines (Requirement 3). Nothing else is changed, so
/// Markdown syntax and tags reach the model verbatim.
pub fn plain_to_spoken(text: &str) -> SpokenScript {
    let mut segments = Vec::new();
    let mut paragraph = String::new();
    let flush = |paragraph: &mut String, segments: &mut Vec<Segment>| {
        let normalized = normalize_whitespace(paragraph);
        paragraph.clear();
        if !normalized.is_empty() {
            segments.push(Segment {
                text: normalized,
                pause_after: Pause::Paragraph,
            });
        }
    };
    for line in text.split('\n') {
        let line = line.replace('\r', "");
        if line.trim().is_empty() {
            flush(&mut paragraph, &mut segments);
        } else {
            paragraph.push_str(&line);
            paragraph.push(' ');
        }
    }
    flush(&mut paragraph, &mut segments);
    SpokenScript { segments }
}

/// Convert Markdown to segments (Requirement 4, RULE-010 to RULE-024).
pub fn markdown_to_spoken(text: &str) -> SpokenScript {
    let body = skip_front_matter(text);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let mut converter = Converter::default();
    for event in Parser::new_ext(body, options) {
        converter.event(event);
    }
    SpokenScript {
        segments: converter.segments,
    }
}

/// RULE-010: skip a leading `---` block that is closed by `---` or `...`.
fn skip_front_matter(text: &str) -> &str {
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return text;
    };
    if first.trim_end() != "---" {
        return text;
    }
    let mut consumed = first.len();
    for line in lines {
        consumed += line.len();
        let marker = line.trim_end();
        if marker == "---" || marker == "..." {
            return &text[consumed..];
        }
    }
    text
}

/// Walks pulldown-cmark events and emits segments.
#[derive(Default)]
struct Converter {
    segments: Vec<Segment>,
    buffer: String,
    html: HtmlText,
    in_code_block: bool,
    in_heading: bool,
    in_autolink: bool,
    footnote_depth: u32,
    row: Option<Vec<String>>,
}

impl Converter {
    fn event(&mut self, event: Event<'_>) {
        if self.footnote_depth > 0 {
            // RULE-023: footnote definitions produce no text.
            match event {
                Event::Start(Tag::FootnoteDefinition(_)) => self.footnote_depth += 1,
                Event::End(TagEnd::FootnoteDefinition) => self.footnote_depth -= 1,
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => self.text(&code),
            Event::Html(html) | Event::InlineHtml(html) => {
                if !self.in_code_block {
                    self.html.feed(&html, &mut self.buffer);
                }
            }
            Event::SoftBreak | Event::HardBreak => self.text(" "),
            // Rules, footnote references, task markers, and math produce no text.
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if self.in_code_block || self.in_autolink || self.html.is_skipping() {
            return;
        }
        self.buffer.push_str(text);
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph
            | Tag::Item
            | Tag::List(_)
            | Tag::BlockQuote(_)
            | Tag::Table(_)
            | Tag::HtmlBlock => {
                self.flush(Pause::Paragraph);
                self.html.reset();
            }
            Tag::Heading { .. } => {
                self.flush(Pause::Paragraph);
                self.in_heading = true;
            }
            Tag::CodeBlock(_) => {
                self.flush(Pause::Paragraph);
                self.in_code_block = true;
            }
            Tag::FootnoteDefinition(_) => {
                self.flush(Pause::Paragraph);
                self.footnote_depth = 1;
            }
            Tag::TableHead | Tag::TableRow => self.row = Some(Vec::new()),
            Tag::Link {
                link_type: LinkType::Autolink | LinkType::Email,
                ..
            } => {
                // RULE-015: autolinks are dropped entirely.
                self.in_autolink = true;
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph
            | TagEnd::Item
            | TagEnd::List(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::HtmlBlock => {
                self.flush(Pause::Paragraph);
                self.html.reset();
            }
            TagEnd::Heading(_) => {
                self.flush(Pause::Heading);
                self.in_heading = false;
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.buffer.clear();
            }
            TagEnd::TableCell => {
                let cell = normalize_whitespace(&self.buffer);
                self.buffer.clear();
                if let Some(row) = self.row.as_mut() {
                    row.push(cell);
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => self.end_row(),
            TagEnd::Link => self.in_autolink = false,
            _ => {}
        }
    }

    /// RULE-021: each non-empty cell is a `Cell` segment, except the row's last, a `Paragraph`.
    fn end_row(&mut self) {
        let Some(row) = self.row.take() else {
            return;
        };
        let cells: Vec<String> = row.into_iter().filter(|c| !c.is_empty()).collect();
        let last = cells.len().saturating_sub(1);
        for (index, text) in cells.into_iter().enumerate() {
            let pause_after = if index == last {
                Pause::Paragraph
            } else {
                Pause::Cell
            };
            self.segments.push(Segment { text, pause_after });
        }
    }

    fn flush(&mut self, pause_after: Pause) {
        let pause_after = if self.in_heading {
            Pause::Heading
        } else {
            pause_after
        };
        let text = normalize_whitespace(&self.buffer);
        self.buffer.clear();
        if !text.is_empty() {
            self.segments.push(Segment { text, pause_after });
        }
    }
}

/// Extracts visible text from raw HTML (RULE-022). State survives across `feed` calls so a
/// comment, a tag, or a `<script>` element can span several events.
#[derive(Default)]
struct HtmlText {
    in_comment: bool,
    tag: Option<String>,
    skipping: Option<String>,
}

impl HtmlText {
    fn reset(&mut self) {
        *self = HtmlText::default();
    }

    fn is_skipping(&self) -> bool {
        self.skipping.is_some()
    }

    fn feed(&mut self, input: &str, out: &mut String) {
        let mut visible = String::new();
        let mut rest = input;
        while !rest.is_empty() {
            if self.in_comment {
                match rest.find("-->") {
                    Some(end) => {
                        self.in_comment = false;
                        rest = &rest[end + 3..];
                    }
                    None => rest = "",
                }
            } else if let Some(tag) = self.tag.as_mut() {
                match rest.find('>') {
                    Some(end) => {
                        tag.push_str(&rest[..end]);
                        let finished = self.tag.take().unwrap_or_default();
                        self.finish_tag(&finished, &mut visible);
                        rest = &rest[end + 1..];
                    }
                    None => {
                        tag.push_str(rest);
                        rest = "";
                    }
                }
            } else if let Some(open) = rest.find('<') {
                if self.skipping.is_none() {
                    visible.push_str(&rest[..open]);
                }
                let after = &rest[open + 1..];
                if let Some(after_marker) = after.strip_prefix("!--") {
                    self.in_comment = true;
                    rest = after_marker;
                } else if after
                    .starts_with(|c: char| c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?'))
                {
                    self.tag = Some(String::new());
                    rest = after;
                } else {
                    if self.skipping.is_none() {
                        visible.push('<');
                    }
                    rest = after;
                }
            } else {
                if self.skipping.is_none() {
                    visible.push_str(rest);
                }
                rest = "";
            }
        }
        out.push_str(&decode_entities(&visible));
    }

    fn finish_tag(&mut self, tag: &str, visible: &mut String) {
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if let Some(skipped) = &self.skipping {
            if closing && *skipped == name {
                self.skipping = None;
            }
            return;
        }
        match name.as_str() {
            "script" | "style" if !closing && !tag.ends_with('/') => self.skipping = Some(name),
            "br" => visible.push(' '),
            _ => {}
        }
    }
}

/// Decode the HTML entities RULE-022 names, including numeric ones.
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let decoded = after
            .find(';')
            .filter(|&end| end <= 10)
            .and_then(|end| entity_char(&after[..end]).map(|c| (c, end)));
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity_char(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse::<u32>().ok()?,
            };
            char::from_u32(code)
        }
    }
}
