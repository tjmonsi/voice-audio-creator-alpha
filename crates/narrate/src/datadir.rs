//! The data directory and the fixed paths inside it (spec-narrate-architecture.md, Data directory
//! layout).

use std::path::{Path, PathBuf};

use crate::error::CliError;

/// `%LOCALAPPDATA%\narrate`, or `NARRATE_DATA_DIR`. Resolving never creates anything.
#[derive(Debug, Clone)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    /// Resolve the directory from `NARRATE_DATA_DIR` (when non-empty) or the local app data folder.
    pub fn resolve() -> Result<DataDir, CliError> {
        let root = match std::env::var_os("NARRATE_DATA_DIR").filter(|v| !v.is_empty()) {
            Some(value) => std::path::absolute(PathBuf::from(value))?,
            None => dirs::data_local_dir()
                .ok_or_else(|| CliError::internal("cannot find the local application data folder"))?
                .join("narrate"),
        };
        Ok(DataDir { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn setup_json(&self) -> PathBuf {
        self.root.join("setup.json")
    }

    pub fn setup_lock(&self) -> PathBuf {
        self.root.join("setup.lock")
    }

    pub fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    pub fn worker_dir(&self) -> PathBuf {
        self.root.join("worker")
    }

    pub fn python_dir(&self) -> PathBuf {
        self.root.join("python")
    }

    pub fn python_cache(&self) -> PathBuf {
        self.root.join("python-cache")
    }

    pub fn uv_cache(&self) -> PathBuf {
        self.root.join("uv-cache")
    }

    pub fn venv(&self) -> PathBuf {
        self.root.join("venv")
    }

    pub fn venv_python(&self) -> PathBuf {
        self.root.join("venv").join("Scripts").join("python.exe")
    }

    pub fn models(&self) -> PathBuf {
        self.root.join("models")
    }

    pub fn voices(&self) -> PathBuf {
        self.root.join("voices")
    }

    pub fn clip(&self, name: &str) -> PathBuf {
        self.voices().join(format!("{name}.wav"))
    }

    pub fn staging(&self) -> PathBuf {
        self.root.join("staging")
    }

    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn hf_home(&self) -> PathBuf {
        self.root.join("hf-home")
    }

    /// Join a pin-file relative path (`/` separated) onto `base`, using native separators.
    pub fn join_pin_path(base: &Path, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(base.to_path_buf(), |path, part| path.join(part))
    }
}
