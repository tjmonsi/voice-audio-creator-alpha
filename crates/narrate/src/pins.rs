//! The pin set: `pins.json`, `pyproject.toml`, `uv.lock`, and the worker script, all embedded in
//! the binary (spec-narrate-setup.md, Pin file schema and Pin set identifier).

use std::path::Path;

use serde::Deserialize;

use crate::error::CliError;
use crate::hashing::sha256_hex;
use crate::hooks::Hooks;
use crate::lockscan::{self, PYTHON_BUILD_PREFIX};

const EMBEDDED_PINS: &str = include_str!("../pins.json");
/// The Python project the worker environment is synced from.
pub const PYPROJECT: &str = include_str!("../../../python/pyproject.toml");
const EMBEDDED_LOCK: &str = include_str!("../../../python/uv.lock");
/// The single-file worker, written to `<data>\worker\<h12>\narrate_worker.py` on first use.
pub const WORKER_SCRIPT: &str = include_str!("../../../python/narrate_worker.py");

/// Directory names (`dir`) of the two Hugging Face repositories.
pub const TURBO_DIR: &str = "chatterbox-turbo";
pub const KOKORO_DIR: &str = "kokoro-82m";

const ALLOWED_REPOS: [&str; 2] = ["ResembleAI/chatterbox-turbo", "hexgrad/Kokoro-82M"];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pins {
    pub format: u32,
    pub uv: UvPin,
    pub python: PythonPin,
    pub huggingface: Vec<HfRepo>,
    pub voices: VoicePins,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UvPin {
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonPin {
    pub version: String,
    #[serde(rename = "build")]
    pub _build: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HfRepo {
    pub repo: String,
    pub revision: String,
    pub dir: String,
    pub files: Vec<HfFile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HfFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoicePins {
    pub reference_text: String,
    pub min_samples: u64,
    pub female: KokoroVoice,
    pub male: KokoroVoice,
    pub builtin: BuiltinVoice,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KokoroVoice {
    #[serde(rename = "engine")]
    pub _engine: String,
    pub voice: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuiltinVoice {
    #[serde(rename = "engine")]
    pub _engine: String,
    pub seed: u64,
}

impl Pins {
    /// The repository whose `dir` is `dir`, if the pins list one.
    pub fn repo_by_dir(&self, dir: &str) -> Option<&HfRepo> {
        self.huggingface.iter().find(|repo| repo.dir == dir)
    }

    /// Check every pin rule of spec-narrate-setup.md, Requirement 1.
    pub fn validate(&self) -> Result<(), CliError> {
        if self.format != 1 {
            return Err(must_be("format", "1", &self.format.to_string()));
        }
        if self.uv.version != "0.10.2" {
            return Err(must_be("uv.version", "0.10.2", &self.uv.version));
        }
        if !self.python.url.starts_with(PYTHON_BUILD_PREFIX) {
            return Err(CliError::source_not_allowed(format!(
                "{} is not an allowed download source",
                self.python.url
            )));
        }
        require_hex("python.sha256", &self.python.sha256, 64)?;
        for (r, repo) in self.huggingface.iter().enumerate() {
            if !ALLOWED_REPOS.contains(&repo.repo.as_str()) {
                return Err(CliError::source_not_allowed(format!(
                    "Hugging Face repository {} is not allowed",
                    repo.repo
                )));
            }
            require_hex(&format!("huggingface[{r}].revision"), &repo.revision, 40)?;
            require_safe_path(&repo.dir)?;
            for (f, file) in repo.files.iter().enumerate() {
                require_safe_path(&file.path)?;
                require_hex(
                    &format!("huggingface[{r}].files[{f}].sha256"),
                    &file.sha256,
                    64,
                )?;
            }
        }
        Ok(())
    }
}

fn must_be(field: &str, expected: &str, found: &str) -> CliError {
    CliError::pin_invalid(format!(
        "pins.json {field} must be {expected}, found {found}"
    ))
}

fn require_hex(field: &str, value: &str, digits: usize) -> Result<(), CliError> {
    let ok = value.len() == digits
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if ok {
        Ok(())
    } else {
        Err(must_be(
            field,
            &format!("{digits} lowercase hex digits"),
            value,
        ))
    }
}

/// A pin path is relative, uses `/` only, and has no empty, `.`, or `..` segment.
fn require_safe_path(value: &str) -> Result<(), CliError> {
    let safe = !value.starts_with('/')
        && !value.contains(['\\', ':'])
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    if safe {
        Ok(())
    } else {
        Err(CliError::pin_invalid(format!(
            "pins.json path {value} is not a safe relative path"
        )))
    }
}

/// The effective pin files of this run and the identifier derived from them.
#[derive(Debug, Clone)]
pub struct PinSet {
    pub pins: Pins,
    pub pins_bytes: Vec<u8>,
    pub lock_bytes: Vec<u8>,
    pub id: String,
}

impl PinSet {
    /// Parse the effective files (embedded, or the test overrides) without applying pin rules.
    pub fn load(hooks: &Hooks) -> Result<PinSet, CliError> {
        let pins_bytes = read_override(hooks.pins_file.as_deref())?
            .unwrap_or_else(|| EMBEDDED_PINS.as_bytes().to_vec());
        let lock_bytes = read_override(hooks.lock_file.as_deref())?
            .unwrap_or_else(|| EMBEDDED_LOCK.as_bytes().to_vec());
        let pins: Pins = serde_json::from_slice(&pins_bytes)
            .map_err(|err| CliError::pin_invalid(format!("pins.json is invalid: {err}")))?;
        let id = pin_set_id(&pins_bytes, PYPROJECT.as_bytes(), &lock_bytes);
        Ok(PinSet {
            pins,
            pins_bytes,
            lock_bytes,
            id,
        })
    }

    /// Apply the allow-list and hash rules to the pins and the lock (setup preflight).
    pub fn validate(&self) -> Result<(), CliError> {
        self.pins.validate()?;
        let lock = String::from_utf8_lossy(&self.lock_bytes);
        lockscan::scan(&lock)
    }
}

fn read_override(path: Option<&Path>) -> Result<Option<Vec<u8>>, CliError> {
    match path {
        Some(path) => std::fs::read(path)
            .map(Some)
            .map_err(|err| CliError::internal(format!("cannot read {}: {err}", path.display()))),
        None => Ok(None),
    }
}

/// `sha256("narrate-pin-set-v1\n" + h(pins) + "\n" + h(pyproject) + "\n" + h(lock) + "\n")`.
pub fn pin_set_id(pins: &[u8], pyproject: &[u8], lock: &[u8]) -> String {
    let text = format!(
        "narrate-pin-set-v1\n{}\n{}\n{}\n",
        sha256_hex(pins),
        sha256_hex(pyproject),
        sha256_hex(lock)
    );
    sha256_hex(text.as_bytes())
}

/// First 12 hex digits of the worker script's sha256, the name of its directory.
pub fn worker_script_dir_name() -> String {
    sha256_hex(WORKER_SCRIPT.as_bytes())[..12].to_owned()
}
