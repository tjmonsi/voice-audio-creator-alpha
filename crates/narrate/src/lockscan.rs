//! Fail-closed line scanner for `uv.lock` (spec-narrate-setup.md, Requirement 1). No TOML parser
//! is used: the scanner understands only the layout uv writes and rejects anything else.

use crate::error::CliError;

/// URL prefixes allowed in `uv.lock` and `pins.json` (the source allow-list; never read from a file).
pub const ALLOWED_URL_PREFIXES: [&str; 5] = [
    "https://pypi.org/simple",
    "https://files.pythonhosted.org/packages/",
    "https://download.pytorch.org/whl/",
    "https://github.com/explosion/spacy-models/releases/download/en_core_web_sm-3.8.0/",
    "https://github.com/astral-sh/python-build-standalone/releases/download/",
];

/// Prefix of the PyTorch explicit index.
pub const PYTORCH_PREFIX: &str = "https://download.pytorch.org/whl/";
/// Prefix of uv-managed Python downloads.
pub const PYTHON_BUILD_PREFIX: &str =
    "https://github.com/astral-sh/python-build-standalone/releases/download/";

const TORCH_PACKAGES: [&str; 2] = ["torch", "torchaudio"];
const URL_STARTS: [&str; 5] = ["https://", "http://", "git+", "file:", "ssh://"];

/// True when `url` starts with an allowed prefix.
pub fn is_allowed_url(url: &str) -> bool {
    ALLOWED_URL_PREFIXES
        .iter()
        .any(|prefix| url.starts_with(prefix))
}

/// Check the lock text against the allow-list and hash rules; the first violation wins.
pub fn scan(lock: &str) -> Result<(), CliError> {
    let mut scanner = Scanner::default();
    for (index, raw) in lock.split('\n').enumerate() {
        let line = raw.trim_end();
        scanner.line(index + 1, line)?;
    }
    scanner.close_block()
}

#[derive(Default)]
struct Block {
    start_line: usize,
    name: Option<String>,
    version: Option<String>,
    source: Option<Source>,
    hashed_artifacts: usize,
    /// URLs of artifact lines (`sdist` and each wheel) in the main part.
    artifact_urls: Vec<String>,
}

enum Source {
    Registry(String),
    Url(String),
    Virtual,
}

#[derive(Default)]
struct Scanner {
    block: Option<Block>,
    /// True while inside the main part of the open `[[package]]` block.
    in_main: bool,
}

impl Scanner {
    fn line(&mut self, number: usize, line: &str) -> Result<(), CliError> {
        let text = line.trim_start();
        if !is_known_shape(text) {
            return Err(CliError::pin_invalid(format!(
                "uv.lock line {number} has an unrecognized form"
            )));
        }
        check_urls(text)?;
        if is_table_header(text) {
            self.close_block()?;
            if text == "[[package]]" {
                self.block = Some(Block {
                    start_line: number,
                    ..Block::default()
                });
                self.in_main = true;
            }
            return Ok(());
        }
        if !self.in_main {
            return Ok(());
        }
        let Some(block) = self.block.as_mut() else {
            return Ok(());
        };
        if let Some(value) = key_value(text, "name") {
            block.name = Some(value);
        } else if let Some(value) = key_value(text, "version") {
            block.version = Some(value);
        } else if text.starts_with("source = ") {
            block.source = Some(parse_source(text, block.name.as_deref())?);
        } else if text.contains("url = \"") {
            let package = block
                .name
                .clone()
                .unwrap_or_else(|| format!("at line {}", block.start_line));
            if !has_sha256(text) {
                return Err(CliError::pin_invalid(format!(
                    "uv.lock line {number} (package {package}) has an artifact without a sha256 hash"
                )));
            }
            block.hashed_artifacts += 1;
            if let Some(url) = quoted_after(text, "url = \"") {
                block.artifact_urls.push(url);
            }
        }
        Ok(())
    }

    /// Apply the block rules (3 completeness, 5, and 6) to the block that just ended.
    fn close_block(&mut self) -> Result<(), CliError> {
        self.in_main = false;
        let Some(block) = self.block.take() else {
            return Ok(());
        };
        let label = block
            .name
            .clone()
            .unwrap_or_else(|| format!("at line {}", block.start_line));
        let missing = |field: &str| {
            CliError::pin_invalid(format!("uv.lock package {label} is missing its {field}"))
        };
        let name = block.name.as_deref().ok_or_else(|| missing("name"))?;
        if block.version.is_none() {
            return Err(missing("version"));
        }
        let source = block.source.as_ref().ok_or_else(|| missing("source"))?;
        match source {
            Source::Virtual if name != "narrate-worker" => {
                return Err(CliError::source_not_allowed(format!(
                    "{name} uses a virtual source, which is not allowed"
                )));
            }
            Source::Registry(_) | Source::Url(_) if block.hashed_artifacts == 0 => {
                return Err(missing("hashed artifact"));
            }
            _ => {}
        }
        check_pytorch_rule(name, source, &block.artifact_urls)
    }
}

/// Rule 6: `torch` and `torchaudio` come from the PyTorch index, and nothing else does.
fn check_pytorch_rule(
    name: &str,
    source: &Source,
    artifact_urls: &[String],
) -> Result<(), CliError> {
    let source_url = match source {
        Source::Registry(url) | Source::Url(url) => Some(url.as_str()),
        Source::Virtual => None,
    };
    if TORCH_PACKAGES.contains(&name) {
        let from_pytorch = |url: &str| url.starts_with(PYTORCH_PREFIX);
        if let Some(url) = source_url.filter(|url| !from_pytorch(url)) {
            return Err(CliError::source_not_allowed(format!(
                "{name} must come from {PYTORCH_PREFIX} (the lock uses {url})"
            )));
        }
        if let Some(url) = artifact_urls.iter().find(|url| !from_pytorch(url)) {
            return Err(CliError::source_not_allowed(format!(
                "{name} must come from {PYTORCH_PREFIX} (the lock uses {url})"
            )));
        }
    } else if source_url
        .into_iter()
        .chain(artifact_urls.iter().map(String::as_str))
        .any(|url| url.starts_with(PYTORCH_PREFIX))
    {
        return Err(CliError::source_not_allowed(format!(
            "{name} may not come from {PYTORCH_PREFIX}"
        )));
    }
    Ok(())
}

/// Rule 1: the line shapes uv writes.
fn is_known_shape(text: &str) -> bool {
    text.is_empty()
        || text.starts_with('#')
        || is_table_header(text)
        || is_key_line(text)
        || text.starts_with('{')
        || text.starts_with('"')
        || text == "]"
        || text == "],"
}

fn is_table_header(text: &str) -> bool {
    let inner = if let Some(rest) = text.strip_prefix("[[") {
        rest.strip_suffix("]]")
    } else {
        text.strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
    };
    inner.is_some_and(|name| {
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    })
}

fn is_key_line(text: &str) -> bool {
    text.split_once(" = ").is_some_and(|(key, value)| {
        !key.is_empty()
            && !value.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    })
}

/// Rule 2: every URL-like substring must use an allowed prefix.
fn check_urls(text: &str) -> Result<(), CliError> {
    let mut rest = text;
    while let Some((start, _)) = URL_STARTS
        .iter()
        .filter_map(|marker| rest.find(marker).map(|at| (at, marker)))
        .min_by_key(|(at, _)| *at)
    {
        let tail = &rest[start..];
        let end = tail.find('"').unwrap_or(tail.len());
        let url = &tail[..end];
        if !is_allowed_url(url) {
            return Err(CliError::source_not_allowed(format!(
                "{url} is not an allowed download source"
            )));
        }
        rest = &tail[end..];
    }
    Ok(())
}

/// Rule 3: parse the `source = { ... }` line.
fn parse_source(text: &str, name: Option<&str>) -> Result<Source, CliError> {
    let package = name.unwrap_or("<unnamed>");
    let inner = text
        .strip_prefix("source = { ")
        .and_then(|rest| rest.strip_suffix(" }"))
        .ok_or_else(|| {
            CliError::pin_invalid(format!(
                "uv.lock package {package} has a source line that is not understood"
            ))
        })?;
    let (key, value) = inner.split_once(" = ").ok_or_else(|| {
        CliError::pin_invalid(format!(
            "uv.lock package {package} has a source line that is not understood"
        ))
    })?;
    let value = value.trim().trim_matches('"').to_owned();
    match key {
        "registry" if is_allowed_url(&value) => Ok(Source::Registry(value)),
        "url" if is_allowed_url(&value) => Ok(Source::Url(value)),
        "registry" | "url" => Err(CliError::source_not_allowed(format!(
            "{value} is not an allowed download source"
        ))),
        "virtual" if value == "." => Ok(Source::Virtual),
        "git" | "path" | "directory" | "editable" | "virtual" => Err(CliError::source_not_allowed(
            format!("{package} uses a {key} source, which is not allowed"),
        )),
        _ => Err(CliError::pin_invalid(format!(
            "uv.lock package {package} has a source line that is not understood"
        ))),
    }
}

fn key_value(text: &str, key: &str) -> Option<String> {
    let value = text
        .strip_prefix(key)?
        .strip_prefix(" = \"")?
        .strip_suffix('"')?;
    Some(value.to_owned())
}

fn quoted_after(text: &str, marker: &str) -> Option<String> {
    let start = text.find(marker)? + marker.len();
    let end = text[start..].find('"')?;
    Some(text[start..start + end].to_owned())
}

/// `hash = "sha256:<64 lowercase hex>"` anywhere on the line.
fn has_sha256(text: &str) -> bool {
    text.find("hash = \"sha256:").is_some_and(|at| {
        let digits = &text[at + "hash = \"sha256:".len()..];
        digits.len() > 64
            && digits.as_bytes()[64] == b'"'
            && digits[..64]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
