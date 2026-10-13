//! 16-bit mono PCM WAV encoding (spec-narrate-generation.md, Requirement 11).

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

/// Turbo `model.sr` (research P1 section 1.4).
pub const SAMPLE_RATE: u32 = 24_000;
/// Size of the RIFF/WAVE header this module writes.
pub const WAV_HEADER_BYTES: u64 = 44;
/// Largest RIFF file size the 32-bit size fields allow.
pub const WAV_SIZE_LIMIT: u64 = 4_294_967_295;

/// Convert a float sample to 16-bit PCM: NaN gives 0, other values are clamped to [-1, 1],
/// scaled by 32767, and rounded half away from zero.
pub fn f32_to_pcm16(sample: f32) -> i16 {
    if sample.is_nan() {
        return 0;
    }
    // The clamped product lies in [-32767.0, 32767.0], so the cast cannot truncate.
    (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

/// Size in bytes of a 16-bit mono WAV file holding `samples` samples.
pub fn wav_file_size(samples: u64) -> u64 {
    WAV_HEADER_BYTES + 2 * samples
}

/// Write a 44-byte header and the little-endian samples, then flush and sync the file.
///
/// Fails with `InvalidInput` when the data would not fit in a RIFF file.
pub fn write_wav_pcm16(path: &Path, sample_rate: u32, samples: &[i16]) -> io::Result<()> {
    let too_large = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "audio exceeds the WAV size limit",
        )
    };
    let data_bytes = u32::try_from(samples.len() as u64 * 2).map_err(|_| too_large())?;
    let riff_bytes = data_bytes.checked_add(36).ok_or_else(too_large)?;
    let mut out = BufWriter::new(File::create(path)?);
    out.write_all(b"RIFF")?;
    out.write_all(&riff_bytes.to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?; // PCM
    out.write_all(&1u16.to_le_bytes())?; // mono
    out.write_all(&sample_rate.to_le_bytes())?;
    out.write_all(&sample_rate.saturating_mul(2).to_le_bytes())?; // bytes per second
    out.write_all(&2u16.to_le_bytes())?; // block align
    out.write_all(&16u16.to_le_bytes())?; // bits per sample
    out.write_all(b"data")?;
    out.write_all(&data_bytes.to_le_bytes())?;
    let mut bytes = Vec::with_capacity(32 * 1024);
    for block in samples.chunks(16 * 1024) {
        bytes.clear();
        for sample in block {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        out.write_all(&bytes)?;
    }
    out.flush()?;
    out.get_ref().sync_all()
}
