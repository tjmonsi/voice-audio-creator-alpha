//! Ctrl+C and Ctrl+Break handling. Only `narrate` reacts to console control events; it stops its
//! children itself (spec-narrate-generation.md Requirement 13).

use std::sync::atomic::{AtomicBool, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Install the console handler. A failure is returned so the caller can report it with
/// `--verbose`; the program still works, it just cannot catch Ctrl+C itself.
pub fn install() -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(|| INTERRUPTED.store(true, Ordering::SeqCst))
}

/// True once Ctrl+C or Ctrl+Break arrived.
pub fn is_set() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}
