//! Text pipeline, WAV encoding, and reference-clip helpers for `narrate`.
//!
//! Everything here is a pure function or a small file helper, so the pipeline runs without the
//! model, a data directory, or the network (FR-005).

pub mod audio;
pub mod chunk;
pub mod clip;
pub mod script;
pub mod spoken;
pub mod tags;

pub use audio::{
    SAMPLE_RATE, WAV_HEADER_BYTES, WAV_SIZE_LIMIT, f32_to_pcm16, wav_file_size, write_wav_pcm16,
};
pub use chunk::{Chunk, MAX_CHUNK_CHARS, chunk_script, split_sentences};
pub use clip::{ClipError, ClipInfo, inspect_clip, is_too_short, needs_trim, write_clip_prefix};
pub use script::{ScriptError, ScriptKind, decode_utf8, load_script};
pub use spoken::{
    Pause, Segment, SpokenScript, markdown_to_spoken, normalize_whitespace, plain_to_spoken,
    to_spoken,
};
pub use tags::{PARALINGUISTIC_TAGS, is_known_tag, unknown_tags};
