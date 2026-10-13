//! Black-box tests for `narrate_core::audio`: PCM conversion, size arithmetic and the WAV
//! header bytes (FR-006, NFR-005).
//!
//! Source: specs/spec-narrate-generation.md (Requirement 11: WAV encoding, RULE-004). No
//! implementation was read.

use narrate_core::audio::{
    f32_to_pcm16, wav_file_size, write_wav_pcm16, SAMPLE_RATE, WAV_HEADER_BYTES, WAV_SIZE_LIMIT,
};

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

// ---------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------

// [Public API: constants | spec-narrate-generation Req 11 | FR-006 | NFR-005]
#[test]
fn constants_have_the_documented_values() {
    assert_eq!(SAMPLE_RATE, 24_000u32);
    assert_eq!(WAV_HEADER_BYTES, 44u64);
    assert_eq!(WAV_SIZE_LIMIT, 4_294_967_295u64);
    assert_eq!(WAV_SIZE_LIMIT, u32::MAX as u64);
}

// [Public API re-exports | spec-narrate-generation Req 11 | FR-006]
#[test]
fn audio_items_are_re_exported_at_the_crate_root() {
    assert_eq!(narrate_core::SAMPLE_RATE, 24_000u32);
    assert_eq!(narrate_core::WAV_HEADER_BYTES, 44u64);
    assert_eq!(narrate_core::WAV_SIZE_LIMIT, 4_294_967_295u64);
    assert_eq!(narrate_core::f32_to_pcm16(0.0), 0);
    assert_eq!(narrate_core::wav_file_size(0), 44);
    let _: fn(&std::path::Path, u32, &[i16]) -> std::io::Result<()> = narrate_core::write_wav_pcm16;
}

// ---------------------------------------------------------------------------------------
// f32_to_pcm16
// ---------------------------------------------------------------------------------------

// [Scenario: Conversion | spec-narrate-generation Req 11 | FR-006]
#[test]
fn f32_to_pcm16_matches_the_spec_vector() {
    let input = [0.0f32, 1.0, -1.0, 1.5, -2.0, 0.5, f32::NAN];
    let expected = [0i16, 32767, -32767, 32767, -32767, 16384, 0];
    let actual: Vec<i16> = input.iter().map(|x| f32_to_pcm16(*x)).collect();
    assert_eq!(actual, expected);
}

// [Public API: f32_to_pcm16 rounding | spec-narrate-generation Req 11 | FR-006]
#[test]
fn f32_to_pcm16_rounds_half_away_from_zero() {
    // 0.5 * 32767 = 16383.5 -> 16384; the mirror image rounds away from zero as well.
    assert_eq!(f32_to_pcm16(0.5), 16384);
    assert_eq!(f32_to_pcm16(-0.5), -16384);
    // 0.25 * 32767 = 8191.75 -> 8192
    assert_eq!(f32_to_pcm16(0.25), 8192);
    assert_eq!(f32_to_pcm16(-0.25), -8192);
}

// [Public API: f32_to_pcm16 boundaries | spec-narrate-generation Req 11 | FR-006]
#[test]
fn f32_to_pcm16_handles_zero_tiny_infinite_and_extreme_values() {
    assert_eq!(f32_to_pcm16(-0.0), 0);
    assert_eq!(f32_to_pcm16(0.00001), 0);
    assert_eq!(f32_to_pcm16(-0.00001), 0);
    assert_eq!(f32_to_pcm16(1.0 / 32767.0), 1);
    assert_eq!(f32_to_pcm16(-1.0 / 32767.0), -1);
    assert_eq!(f32_to_pcm16(f32::INFINITY), 32767);
    assert_eq!(f32_to_pcm16(f32::NEG_INFINITY), -32767);
    assert_eq!(f32_to_pcm16(f32::MAX), 32767);
    assert_eq!(f32_to_pcm16(f32::MIN), -32767);
    assert_eq!(f32_to_pcm16(-f32::NAN), 0);
}

// [Public API: f32_to_pcm16 range | spec-narrate-generation Req 11 | FR-006]
#[test]
fn f32_to_pcm16_never_returns_the_minimum_i16_value() {
    for i in -2000..=2000 {
        let x = i as f32 / 1000.0;
        let v = f32_to_pcm16(x);
        assert!(v >= -32767, "{x} gave {v}");
    }
}

// ---------------------------------------------------------------------------------------
// wav_file_size
// ---------------------------------------------------------------------------------------

// [Public API: wav_file_size | spec-narrate-generation Req 11 | FR-006 | NFR-005 | RULE-004]
#[test]
fn wav_file_size_is_header_plus_two_bytes_per_sample() {
    assert_eq!(wav_file_size(0), 44);
    assert_eq!(wav_file_size(1), 46);
    assert_eq!(wav_file_size(24_000), 44 + 48_000);
    // RULE-004 example: limit 1000 allows at most 478 samples.
    assert_eq!(wav_file_size(478), 1000);
    assert_eq!(wav_file_size(479), 1002);
}

// [Public API: wav_file_size, large values | spec-narrate-generation Req 11 | NFR-005]
#[test]
fn wav_file_size_does_not_overflow_near_the_u32_limit() {
    let samples = u32::MAX as u64;
    assert_eq!(wav_file_size(samples), 44 + 2 * samples);
    assert!(wav_file_size(samples) > WAV_SIZE_LIMIT);
    // Largest sample count that still fits the RIFF size limit.
    let max_fit = (WAV_SIZE_LIMIT - WAV_HEADER_BYTES) / 2;
    assert!(wav_file_size(max_fit) <= WAV_SIZE_LIMIT);
    assert!(wav_file_size(max_fit + 1) > WAV_SIZE_LIMIT);
}

// ---------------------------------------------------------------------------------------
// write_wav_pcm16
// ---------------------------------------------------------------------------------------

// [Scenario: Format | spec-narrate-generation Req 11 | FR-006]
#[test]
fn write_wav_pcm16_writes_the_documented_header_and_little_endian_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.wav");
    let samples: Vec<i16> = vec![0, 1, -1, 32767, -32767, 256, -2];
    write_wav_pcm16(&path, 24_000, &samples).unwrap();
    let bytes = std::fs::read(&path).unwrap();

    assert_eq!(bytes.len() as u64, wav_file_size(samples.len() as u64));
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(u32_at(&bytes, 4) as usize, bytes.len() - 8);
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(&bytes[12..16], b"fmt ");
    assert_eq!(u32_at(&bytes, 16), 16, "fmt chunk length");
    assert_eq!(u16_at(&bytes, 20), 1, "format tag PCM");
    assert_eq!(u16_at(&bytes, 22), 1, "channels");
    assert_eq!(u32_at(&bytes, 24), 24_000, "sample rate");
    assert_eq!(u32_at(&bytes, 28), 48_000, "bytes per second");
    assert_eq!(u16_at(&bytes, 32), 2, "block align");
    assert_eq!(u16_at(&bytes, 34), 16, "bits per sample");
    assert_eq!(&bytes[36..40], b"data", "data chunk follows fmt directly");
    assert_eq!(u32_at(&bytes, 40) as usize, samples.len() * 2);
    let mut expected = Vec::new();
    for s in &samples {
        expected.extend_from_slice(&s.to_le_bytes());
    }
    assert_eq!(&bytes[44..], &expected[..]);
}

// [Scenario: Format | spec-narrate-generation Req 11 | FR-006]
#[test]
fn write_wav_pcm16_with_no_samples_writes_only_the_header() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.wav");
    write_wav_pcm16(&path, 24_000, &[]).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.len(), 44);
    assert_eq!(&bytes[36..40], b"data");
    assert_eq!(u32_at(&bytes, 40), 0);
    assert_eq!(u32_at(&bytes, 4), 36);
}

// [Public API: write_wav_pcm16 sample_rate argument | spec-narrate-generation Req 11 | FR-006]
#[test]
fn write_wav_pcm16_uses_the_given_sample_rate_in_the_header() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rate.wav");
    write_wav_pcm16(&path, 16_000, &[1, 2, 3]).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(u32_at(&bytes, 24), 16_000);
    assert_eq!(u32_at(&bytes, 28), 32_000);
}

// [Public API: write_wav_pcm16 truncates | spec-narrate-generation Req 11 | FR-006]
#[test]
fn write_wav_pcm16_creates_or_truncates_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.wav");
    std::fs::write(&path, vec![0xAAu8; 5000]).unwrap();
    write_wav_pcm16(&path, 24_000, &[7, 8]).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.len(), 48);
    assert_eq!(&bytes[0..4], b"RIFF");
}

// [Public API: write_wav_pcm16 errors | spec-narrate-generation Req 11 | FR-006]
#[test]
fn write_wav_pcm16_reports_io_error_when_the_directory_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("no-such-dir").join("x.wav");
    assert!(write_wav_pcm16(&path, 24_000, &[1]).is_err());
}

// [Scenario: Format | spec-narrate-generation Req 11 | FR-006]
#[test]
fn write_wav_pcm16_round_trips_a_large_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.wav");
    let samples: Vec<i16> = (0..100_000i32).map(|i| ((i * 31) % 65_535 - 32_767) as i16).collect();
    write_wav_pcm16(&path, 24_000, &samples).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.len() as u64, wav_file_size(samples.len() as u64));
    let decoded: Vec<i16> = bytes[44..]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    assert_eq!(decoded, samples);
}
