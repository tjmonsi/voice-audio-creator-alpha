//! File helpers: atomic writes, the per-run temporary directory, and stale-file cleanup.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::datadir::DataDir;

/// Milliseconds since the Unix epoch.
pub fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// Seconds since the Unix epoch.
pub fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Write `bytes` to `path` through a temporary file and a rename, creating parent directories.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = sibling_temp(path);
    let written = fs::write(&temp, bytes).and_then(|()| fs::rename(&temp, path));
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// Like [`write_atomic`], but leaves the file alone when it already holds `bytes`.
pub fn write_atomic_if_changed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match fs::read(path) {
        Ok(existing) if existing == bytes => Ok(()),
        _ => write_atomic(path, bytes),
    }
}

fn sibling_temp(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(format!(".narrate-{}.tmp", std::process::id()));
    path.with_file_name(name)
}

/// `D\tmp\<pid>-<start ms>`: `TEMP` and `TMP` of every child of this run. Removed on drop, together
/// with `D\tmp` when that is left empty.
pub struct RunTemp {
    dir: PathBuf,
    parent: PathBuf,
}

impl RunTemp {
    /// Create the directory; the data directory must already exist.
    pub fn create(data: &DataDir) -> io::Result<RunTemp> {
        let parent = data.tmp();
        let dir = parent.join(format!("{}-{}", std::process::id(), unix_millis()));
        fs::create_dir_all(&dir)?;
        Ok(RunTemp { dir, parent })
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for RunTemp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
        let _ = fs::remove_dir(&self.parent);
    }
}

/// True when `path` was last written more than `age` ago.
fn older_than(path: &Path, age: Duration) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|elapsed| elapsed > age)
}

fn remove_any(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

/// Delete entries of `D\tmp` older than 24 hours. Never creates `D\tmp`.
pub fn clean_stale_tmp(data: &DataDir) {
    let Ok(entries) = fs::read_dir(data.tmp()) else {
        return;
    };
    for entry in entries.flatten() {
        if older_than(&entry.path(), Duration::from_secs(24 * 3600)) {
            remove_any(&entry.path());
        }
    }
}

/// Delete `staging\voice-*.wav` files older than one hour (trimmed clips a killed run left behind).
pub fn clean_stale_staging_clips(data: &DataDir) {
    let Ok(entries) = fs::read_dir(data.staging()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.starts_with("voice-")
            && name.ends_with(".wav")
            && older_than(&entry.path(), Duration::from_secs(3600))
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Delete `logs\worker-*.log` files older than seven days (RULE-007).
pub fn clean_old_worker_logs(data: &DataDir) {
    let Ok(entries) = fs::read_dir(data.logs()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.starts_with("worker-")
            && name.ends_with(".log")
            && older_than(&entry.path(), Duration::from_secs(7 * 24 * 3600))
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}
