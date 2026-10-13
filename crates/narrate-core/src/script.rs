//! Script loading and decoding (spec-narrate-text-pipeline.md, Requirements 1 and 2).

use std::io;
use std::path::{Path, PathBuf};

use crate::spoken::{SpokenScript, to_spoken};

/// How a script file is converted to speech, decided by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptKind {
    /// `.md` and `.markdown`.
    Markdown,
    /// `.txt`.
    PlainText,
}

/// Why a script could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// The extension is not `.md`, `.markdown`, or `.txt`.
    #[error("unsupported script type: {}", path.display())]
    UnsupportedExtension { path: PathBuf },
    /// The path does not exist.
    #[error("input file not found: {}", path.display())]
    NotFound { path: PathBuf },
    /// The path exists but is not a file.
    #[error("input path is not a file: {}", path.display())]
    NotAFile { path: PathBuf },
    /// Reading the file failed.
    #[error("cannot read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The bytes are not valid UTF-8; `offset` is the first invalid byte of the original file.
    #[error("{} is not valid UTF-8 (invalid byte at offset {offset})", path.display())]
    InvalidUtf8 { path: PathBuf, offset: usize },
    /// The script has no speakable text after conversion.
    #[error("{} has no speakable text after conversion", path.display())]
    Empty { path: PathBuf },
}

impl ScriptKind {
    /// Classify `path` by extension, comparing ASCII case-insensitively.
    pub fn from_path(path: &Path) -> Result<ScriptKind, ScriptError> {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("md" | "markdown") => Ok(ScriptKind::Markdown),
            Some("txt") => Ok(ScriptKind::PlainText),
            _ => Err(ScriptError::UnsupportedExtension {
                path: path.to_path_buf(),
            }),
        }
    }
}

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Remove one leading BOM and validate the rest as UTF-8 (RULE-002).
pub fn decode_utf8(path: &Path, bytes: &[u8]) -> Result<String, ScriptError> {
    let (body, base) = match bytes.strip_prefix(&BOM) {
        Some(rest) => (rest, BOM.len()),
        None => (bytes, 0),
    };
    match std::str::from_utf8(body) {
        Ok(text) => Ok(text.to_owned()),
        Err(err) => Err(ScriptError::InvalidUtf8 {
            path: path.to_path_buf(),
            offset: base + err.valid_up_to(),
        }),
    }
}

/// Read, decode, and convert a script file; fails with `Empty` when nothing is speakable.
pub fn load_script(path: &Path) -> Result<SpokenScript, ScriptError> {
    let kind = ScriptKind::from_path(path)?;
    let metadata = std::fs::metadata(path).map_err(|source| match source.kind() {
        io::ErrorKind::NotFound => ScriptError::NotFound {
            path: path.to_path_buf(),
        },
        _ => ScriptError::Io {
            path: path.to_path_buf(),
            source,
        },
    })?;
    if !metadata.is_file() {
        return Err(ScriptError::NotAFile {
            path: path.to_path_buf(),
        });
    }
    let bytes = std::fs::read(path).map_err(|source| ScriptError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let text = decode_utf8(path, &bytes)?;
    let script = to_spoken(&text, kind);
    if script.is_empty() {
        return Err(ScriptError::Empty {
            path: path.to_path_buf(),
        });
    }
    Ok(script)
}
