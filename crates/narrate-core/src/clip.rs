//! Reference-clip inspection and trimming (spec-narrate-generation.md, Requirement 3).

use std::fs::File;
use std::io::{self, BufReader, Read, Seek};
use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader, WavWriter};

/// Shape of a decoded WAV clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipInfo {
    /// Samples per second per channel.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Bits per stored sample.
    pub bits_per_sample: u16,
    /// True for IEEE float samples.
    pub is_float: bool,
    /// Samples per channel.
    pub frames: u64,
}

impl ClipInfo {
    /// Clip length in seconds.
    pub fn duration_secs(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate)
    }
}

/// Why a clip cannot be used. `path` is always the path exactly as the caller passed it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClipError {
    /// The file does not exist.
    #[error("voice file not found: {}", path.display())]
    NotFound { path: PathBuf },
    /// Not a readable WAV file (no RIFF/WAVE header, malformed, or truncated).
    #[error("{} is not a readable WAV file ({detail})", path.display())]
    Unreadable { path: PathBuf, detail: String },
    /// A valid WAV file with an encoding this program does not read.
    #[error(
        "{} uses an unsupported WAV encoding ({detail}); supported encodings: PCM 8, 16, 24, or 32-bit integer, or 32-bit float",
        path.display()
    )]
    Unsupported { path: PathBuf, detail: String },
}

/// True when the clip is 5.0 s or shorter, which the model rejects.
pub fn is_too_short(info: &ClipInfo) -> bool {
    info.frames <= 5 * u64::from(info.sample_rate)
}

/// True when the clip is longer than the 10 s the decoder conditioning uses.
pub fn needs_trim(info: &ClipInfo) -> bool {
    info.frames > 10 * u64::from(info.sample_rate)
}

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// Read every sample of `path` and report its shape.
pub fn inspect_clip(path: &Path) -> Result<ClipInfo, ClipError> {
    let (mut reader, info) = open_clip(path)?;
    let spec = reader.spec();
    let read = match spec.sample_format {
        SampleFormat::Float => count_samples(reader.samples::<f32>()),
        SampleFormat::Int => count_samples(reader.samples::<i32>()),
    };
    let total = read.map_err(|err| unreadable(path, err))?;
    Ok(ClipInfo {
        frames: total / u64::from(spec.channels),
        ..info
    })
}

/// Write the first `max_frames` frames of `src` to `dst` in the same sample format.
pub fn write_clip_prefix(src: &Path, dst: &Path, max_frames: u64) -> Result<ClipInfo, ClipError> {
    let (mut reader, info) = open_clip(src)?;
    let spec = reader.spec();
    let wanted = max_frames.saturating_mul(u64::from(spec.channels));
    let write_err = |err: hound::Error| ClipError::Unreadable {
        path: dst.to_path_buf(),
        detail: format!("cannot write the trimmed clip: {err}"),
    };
    let mut writer = WavWriter::create(dst, spec).map_err(write_err)?;
    let mut written = 0u64;
    match spec.sample_format {
        SampleFormat::Float => {
            for sample in reader
                .samples::<f32>()
                .take(usize::try_from(wanted).unwrap_or(usize::MAX))
            {
                writer
                    .write_sample(sample.map_err(|e| unreadable(src, e))?)
                    .map_err(write_err)?;
                written += 1;
            }
        }
        SampleFormat::Int => {
            for sample in reader
                .samples::<i32>()
                .take(usize::try_from(wanted).unwrap_or(usize::MAX))
            {
                writer
                    .write_sample(sample.map_err(|e| unreadable(src, e))?)
                    .map_err(write_err)?;
                written += 1;
            }
        }
    }
    writer.finalize().map_err(write_err)?;
    Ok(ClipInfo {
        frames: written / u64::from(spec.channels),
        ..info
    })
}

fn count_samples<T>(
    samples: impl Iterator<Item = Result<T, hound::Error>>,
) -> Result<u64, hound::Error> {
    let mut total = 0u64;
    for sample in samples {
        sample?;
        total += 1;
    }
    Ok(total)
}

fn unreadable(path: &Path, err: hound::Error) -> ClipError {
    ClipError::Unreadable {
        path: path.to_path_buf(),
        detail: err.to_string(),
    }
}

/// Validate the `fmt ` chunk ourselves, then open the file with `hound`.
fn open_clip(path: &Path) -> Result<(WavReader<BufReader<File>>, ClipInfo), ClipError> {
    let file = File::open(path).map_err(|err| match err.kind() {
        io::ErrorKind::NotFound => ClipError::NotFound {
            path: path.to_path_buf(),
        },
        _ => ClipError::Unreadable {
            path: path.to_path_buf(),
            detail: err.to_string(),
        },
    })?;
    let fmt = read_fmt_chunk(&file).map_err(|detail| ClipError::Unreadable {
        path: path.to_path_buf(),
        detail,
    })?;
    if !is_supported(&fmt) {
        return Err(ClipError::Unsupported {
            path: path.to_path_buf(),
            detail: format!(
                "format tag {}, {} bits",
                fmt.format_tag, fmt.bits_per_sample
            ),
        });
    }
    (&file).rewind().map_err(|err| ClipError::Unreadable {
        path: path.to_path_buf(),
        detail: err.to_string(),
    })?;
    let reader = WavReader::new(BufReader::new(file)).map_err(|err| unreadable(path, err))?;
    let spec = reader.spec();
    let info = ClipInfo {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        bits_per_sample: spec.bits_per_sample,
        is_float: spec.sample_format == SampleFormat::Float,
        frames: u64::from(reader.duration()),
    };
    if info.channels == 0 || info.sample_rate == 0 {
        return Err(ClipError::Unreadable {
            path: path.to_path_buf(),
            detail: "the header declares no channels or no sample rate".to_owned(),
        });
    }
    Ok((reader, info))
}

struct FmtChunk {
    format_tag: u16,
    bits_per_sample: u16,
    /// SubFormat tag of a `WAVE_FORMAT_EXTENSIBLE` chunk.
    sub_format: Option<u16>,
}

fn is_supported(fmt: &FmtChunk) -> bool {
    let tag = if fmt.format_tag == WAVE_FORMAT_EXTENSIBLE {
        match fmt.sub_format {
            Some(sub) => sub,
            None => return false,
        }
    } else {
        fmt.format_tag
    };
    match tag {
        WAVE_FORMAT_PCM => matches!(fmt.bits_per_sample, 8 | 16 | 24 | 32),
        WAVE_FORMAT_IEEE_FLOAT => fmt.bits_per_sample == 32,
        _ => false,
    }
}

/// Walk the RIFF chunks up to `fmt ` without decoding any audio.
fn read_fmt_chunk(file: &File) -> Result<FmtChunk, String> {
    let mut input = BufReader::new(file);
    let mut header = [0u8; 12];
    input
        .read_exact(&mut header)
        .map_err(|_| "the file is too short to be a WAV file".to_owned())?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".to_owned());
    }
    loop {
        let mut chunk_header = [0u8; 8];
        input
            .read_exact(&mut chunk_header)
            .map_err(|_| "the file ends before the fmt chunk".to_owned())?;
        let size = u32::from_le_bytes([
            chunk_header[4],
            chunk_header[5],
            chunk_header[6],
            chunk_header[7],
        ]);
        if &chunk_header[0..4] == b"fmt " {
            if size < 16 {
                return Err("the fmt chunk is too small".to_owned());
            }
            let mut body = vec![0u8; size.min(64) as usize];
            input
                .read_exact(&mut body)
                .map_err(|_| "the fmt chunk is truncated".to_owned())?;
            let format_tag = u16::from_le_bytes([body[0], body[1]]);
            let bits_per_sample = u16::from_le_bytes([body[14], body[15]]);
            let sub_format = (format_tag == WAVE_FORMAT_EXTENSIBLE && body.len() >= 26)
                .then(|| u16::from_le_bytes([body[24], body[25]]));
            return Ok(FmtChunk {
                format_tag,
                bits_per_sample,
                sub_format,
            });
        }
        // Chunks are word aligned: skip the body and one pad byte when the size is odd.
        let skip = u64::from(size) + u64::from(size & 1);
        let copied = io::copy(&mut (&mut input).take(skip), &mut io::sink())
            .map_err(|err| format!("cannot read the file: {err}"))?;
        if copied < skip {
            return Err("the file ends before the fmt chunk".to_owned());
        }
    }
}
