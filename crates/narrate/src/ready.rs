//! Readiness: which parts of the data directory exist, and which pin set built it
//! (spec-narrate-setup.md, Requirement 7). Existence only; hashes are checked during setup.

use std::path::Path;

use serde::Deserialize;

use crate::datadir::DataDir;
use crate::error::CliError;
use crate::pins::{KOKORO_DIR, PinSet, TURBO_DIR};

/// The chatterbox-tts version the environment must hold.
pub const PACKAGE_VERSION: &str = "0.1.7";

/// State of the `package` component.
#[derive(Debug, PartialEq, Eq)]
pub enum Package {
    Present,
    Missing,
    Wrong(String),
}

/// What `setup.json` says about the pin set.
#[derive(Debug, PartialEq, Eq)]
pub enum Record {
    /// No readable record of format 1.
    Missing,
    Recorded(String),
}

pub struct Readiness {
    pub environment: bool,
    pub package: Package,
    pub turbo_missing: Vec<String>,
    pub kokoro_missing: Vec<String>,
    pub female: bool,
    pub male: bool,
    pub builtin: bool,
    pub record: Record,
    pub binary_id: String,
}

#[derive(Deserialize)]
struct SetupRecord {
    format: u32,
    pin_set_id: String,
}

/// Read `setup.json`; unknown fields are ignored.
pub fn read_record(data: &DataDir) -> Record {
    let parsed = std::fs::read(data.setup_json())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SetupRecord>(&bytes).ok());
    match parsed {
        Some(record) if record.format == 1 => Record::Recorded(record.pin_set_id),
        _ => Record::Missing,
    }
}

impl Readiness {
    pub fn check(data: &DataDir, pins: &PinSet) -> Readiness {
        let missing_files = |dir: &str| -> Vec<String> {
            let Some(repo) = pins.pins.repo_by_dir(dir) else {
                return Vec::new();
            };
            let base = data.models().join(dir);
            repo.files
                .iter()
                .filter(|file| !DataDir::join_pin_path(&base, &file.path).is_file())
                .map(|file| file.path.clone())
                .collect()
        };
        Readiness {
            environment: data.venv().join("pyvenv.cfg").is_file() && data.venv_python().is_file(),
            package: package_state(data),
            turbo_missing: missing_files(TURBO_DIR),
            kokoro_missing: missing_files(KOKORO_DIR),
            female: data.clip("female").is_file(),
            male: data.clip("male").is_file(),
            builtin: data.clip("builtin").is_file(),
            record: read_record(data),
            binary_id: pins.id.clone(),
        }
    }

    /// Labels of the missing components, in the order of the readiness table.
    pub fn missing_components(&self) -> Vec<String> {
        let mut missing = Vec::new();
        if !self.environment {
            missing.push("environment missing".to_owned());
        }
        if self.package != Package::Present {
            missing.push("package missing".to_owned());
        }
        if !self.turbo_missing.is_empty() {
            missing.push(format!(
                "turbo weights missing ({})",
                self.turbo_missing.join(", ")
            ));
        }
        if !self.kokoro_missing.is_empty() {
            missing.push(format!(
                "kokoro weights missing ({})",
                self.kokoro_missing.join(", ")
            ));
        }
        for (present, label) in [
            (self.female, "female clip"),
            (self.male, "male clip"),
            (self.builtin, "builtin clip"),
        ] {
            if !present {
                missing.push(format!("{label} missing"));
            }
        }
        missing
    }

    pub fn pin_matches(&self) -> bool {
        self.record == Record::Recorded(self.binary_id.clone())
    }

    pub fn is_ready(&self) -> bool {
        self.missing_components().is_empty() && self.pin_matches()
    }

    /// The generation check: the first failing category becomes the error.
    pub fn require_ready(&self, data: &DataDir) -> Result<(), CliError> {
        let missing = self.missing_components();
        if !missing.is_empty() {
            return Err(CliError::not_set_up(&missing.join(", ")));
        }
        match &self.record {
            Record::Missing => Err(CliError::not_set_up("setup record missing")),
            Record::Recorded(id) if *id != self.binary_id => {
                Err(CliError::pin_mismatch(data.root(), id, &self.binary_id))
            }
            Record::Recorded(_) => Ok(()),
        }
    }
}

fn package_state(data: &DataDir) -> Package {
    let site = data.venv().join("Lib").join("site-packages");
    if site
        .join(format!("chatterbox_tts-{PACKAGE_VERSION}.dist-info"))
        .join("METADATA")
        .is_file()
    {
        return Package::Present;
    }
    wrong_version(&site).map_or(Package::Missing, Package::Wrong)
}

/// The version of another `chatterbox_tts-*.dist-info` directory, if one exists.
fn wrong_version(site: &Path) -> Option<String> {
    std::fs::read_dir(site).ok()?.flatten().find_map(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        name.strip_prefix("chatterbox_tts-")?
            .strip_suffix(".dist-info")
            .filter(|version| *version != PACKAGE_VERSION)
            .map(str::to_owned)
    })
}
