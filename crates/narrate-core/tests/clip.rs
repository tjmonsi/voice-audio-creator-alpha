//! Black-box tests for `narrate_core::clip`: `inspect_clip`, `is_too_short`, `needs_trim`,
//! `write_clip_prefix` (FR-009).
//!
//! Source: specs/spec-narrate-generation.md (Requirement 3: Custom voice clip validation,
//! `narrate_core::clip` API). WAV fixtures are built byte by byte in this file so no decoder
//! behavior is assumed. No implementation was read.
//!
//! ClipError comparison rules from the spec: `==` only for `NotFound` and `Unsupported`;
//! `matches!` for `Unreadable` (its detail is free text).

use std::path::{Path, PathBuf};

use narrate_core::clip::{
    inspect_clip, is_too_short, needs_trim, write_clip_prefix, ClipError, ClipInfo,
};

// ---------------------------------------------------------------------------------------
// WAV fixture builders
// ---------------------------------------------------------------------------------------

fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(id);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn fmt16(tag: u16, channels: u16, rate: u32, bits: u16) -> Vec<u8> {
    let block_align = channels * bits.div_ceil(8);
    let mut b = Vec::new();
    b.extend_from_slice(&tag.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * block_align as u32).to_le_bytes());
    b.extend_from_slice(&block_align.to_le_bytes());
    b.extend_from_slice(&bits.to_le_bytes());
    b
}

fn fmt_extensible(sub_format_code: u16, channels: u16, rate: u32, bits: u16) -> Vec<u8> {
    let block_align = channels * bits.div_ceil(8);
    let mut b = Vec::new();
    b.extend_from_slice(&0xFFFEu16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * block_align as u32).to_le_bytes());
    b.extend_from_slice(&block_align.to_le_bytes());
    b.extend_from_slice(&bits.to_le_bytes());
    b.extend_from_slice(&22u16.to_le_bytes()); // cbSize
    b.extend_from_slice(&bits.to_le_bytes()); // valid bits per sample
    b.extend_from_slice(&0u32.to_le_bytes()); // channel mask
    // SubFormat GUID: code, then 00 00 00 00 10 00 80 00 00 AA 00 38 9B 71.
    b.extend_from_slice(&sub_format_code.to_le_bytes());
    b.extend_from_slice(&[0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71]);
    b
}

fn riff(fmt_body: &[u8], data: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    body.extend_from_slice(&chunk(b"fmt ", fmt_body));
    body.extend_from_slice(&chunk(b"data", data));
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// A WAV with `frames` frames; data bytes are a repeating ramp so prefix copies can be compared
/// byte for byte.
fn wav(tag: u16, channels: u16, rate: u32, bits: u16, frames: u64) -> Vec<u8> {
    let block_align = channels as u64 * bits.div_ceil(8) as u64;
    let data: Vec<u8> = (0..frames * block_align).map(|i| (i % 251) as u8).collect();
    riff(&fmt16(tag, channels, rate, bits), &data)
}

fn pcm16_mono_24k(frames: u64) -> Vec<u8> {
    wav(1, 1, 24_000, 16, frames)
}

fn data_chunk(bytes: &[u8]) -> Vec<u8> {
    // Walk the RIFF chunks and return the body of the `data` chunk.
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size =
            u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        if id == b"data" {
            return bytes[at + 8..at + 8 + size].to_vec();
        }
        at += 8 + size + (size % 2);
    }
    panic!("no data chunk");
}

fn put(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.path().join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

fn info(rate: u32, frames: u64) -> ClipInfo {
    ClipInfo { sample_rate: rate, channels: 1, bits_per_sample: 16, is_float: false, frames }
}

fn assert_is_error<E: std::error::Error + Clone + PartialEq>() {}

// ---------------------------------------------------------------------------------------
// inspect_clip: supported encodings
// ---------------------------------------------------------------------------------------

// [Scenario: Valid short clip | spec-narrate-generation Req 3 | FR-009 | UR-002]
#[test]
fn inspect_clip_reports_pcm16_mono_24k_details() {
    let dir = tempfile::tempdir().unwrap();
    let p = put(&dir, "my.wav", &pcm16_mono_24k(168_000)); // 7.0 s
    let got = inspect_clip(&p).unwrap();
    assert_eq!(
        got,
        ClipInfo {
            sample_rate: 24_000,
            channels: 1,
            bits_per_sample: 16,
            is_float: false,
            frames: 168_000
        }
    );
    assert!((got.duration_secs() - 7.0).abs() < 1e-9);
}

// [Requirement 3: any channel count and sample rate | spec-narrate-generation | FR-009]
#[test]
fn inspect_clip_accepts_each_supported_pcm_depth_float_and_channel_layout() {
    let dir = tempfile::tempdir().unwrap();
    // (name, tag, channels, rate, bits, frames, expected is_float)
    let cases: &[(&str, u16, u16, u32, u16, u64, bool)] = &[
        ("pcm8.wav", 1, 1, 8_000, 8, 16_000, false),
        ("pcm16-stereo.wav", 1, 2, 44_100, 16, 44_100, false),
        ("pcm24.wav", 1, 1, 48_000, 24, 48_000, false),
        ("pcm32.wav", 1, 1, 96_000, 32, 1_000, false),
        ("float32.wav", 3, 1, 22_050, 32, 22_050, true),
        ("float32-6ch.wav", 3, 6, 48_000, 32, 4_800, true),
    ];
    for (name, tag, ch, rate, bits, frames, is_float) in cases {
        let p = put(&dir, name, &wav(*tag, *ch, *rate, *bits, *frames));
        let got = inspect_clip(&p).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(
            got,
            ClipInfo {
                sample_rate: *rate,
                channels: *ch,
                bits_per_sample: *bits,
                is_float: *is_float,
                frames: *frames
            },
            "{name}"
        );
    }
}

// [Requirement 3: WAVE_FORMAT_EXTENSIBLE | spec-narrate-generation | FR-009]
#[test]
fn inspect_clip_accepts_extensible_pcm_and_extensible_float() {
    let dir = tempfile::tempdir().unwrap();
    let frames = 1_000u64;
    let pcm = riff(&fmt_extensible(1, 2, 48_000, 16), &vec![0u8; (frames * 4) as usize]);
    let p = put(&dir, "ext-pcm.wav", &pcm);
    let got = inspect_clip(&p).unwrap();
    assert_eq!((got.channels, got.bits_per_sample, got.is_float, got.frames), (2, 16, false, frames));

    let flt = riff(&fmt_extensible(3, 1, 48_000, 32), &vec![0u8; (frames * 4) as usize]);
    let p = put(&dir, "ext-float.wav", &flt);
    let got = inspect_clip(&p).unwrap();
    assert_eq!((got.channels, got.bits_per_sample, got.is_float, got.frames), (1, 32, true, frames));
}

// [Requirement 3: zero-length audio | spec-narrate-generation | FR-009]
#[test]
fn inspect_clip_reads_a_header_only_file_as_zero_frames() {
    let dir = tempfile::tempdir().unwrap();
    let p = put(&dir, "empty-data.wav", &pcm16_mono_24k(0));
    let got = inspect_clip(&p).unwrap();
    assert_eq!(got.frames, 0);
    assert!(is_too_short(&got));
}

// [Requirement 3: path carried as passed | spec-narrate-generation | FR-009]
#[test]
fn inspect_clip_errors_carry_the_path_exactly_as_passed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    // `..` is preserved by component comparison, so equality proves no normalization happened.
    let odd = dir.path().join("sub").join("..").join("alaw.wav");
    std::fs::write(&odd, wav(6, 1, 8_000, 8, 100)).unwrap();
    match inspect_clip(&odd) {
        Err(ClipError::Unsupported { path, .. }) => assert_eq!(path, odd),
        other => panic!("expected Unsupported, got {other:?}"),
    }
    let missing = dir.path().join("sub").join("..").join("missing.wav");
    assert_eq!(inspect_clip(&missing), Err(ClipError::NotFound { path: missing.clone() }));
}

// ---------------------------------------------------------------------------------------
// inspect_clip: unsupported encodings
// ---------------------------------------------------------------------------------------

// [Scenario: Unsupported encoding | spec-narrate-generation Req 3 | FR-009]
#[test]
fn a_law_clip_is_unsupported_with_the_exact_detail() {
    let dir = tempfile::tempdir().unwrap();
    let p = put(&dir, "odd.wav", &wav(6, 1, 8_000, 8, 8_000));
    assert_eq!(
        inspect_clip(&p),
        Err(ClipError::Unsupported { path: p.clone(), detail: "format tag 6, 8 bits".to_string() })
    );
}

// [Requirement 3: any other pair is Unsupported | spec-narrate-generation | FR-009]
#[test]
fn other_unsupported_pairs_report_tag_and_bits_in_decimal() {
    let dir = tempfile::tempdir().unwrap();
    // (tag, bits, expected detail)
    let cases: &[(u16, u16, &str)] = &[
        (7, 8, "format tag 7, 8 bits"),
        (2, 4, "format tag 2, 4 bits"),
        (3, 64, "format tag 3, 64 bits"),
        (1, 64, "format tag 1, 64 bits"),
        (1, 12, "format tag 1, 12 bits"),
        (3, 16, "format tag 3, 16 bits"),
    ];
    for (i, (tag, bits, detail)) in cases.iter().enumerate() {
        let p = put(&dir, &format!("odd-{i}.wav"), &wav(*tag, 1, 8_000, *bits, 100));
        assert_eq!(
            inspect_clip(&p),
            Err(ClipError::Unsupported { path: p.clone(), detail: detail.to_string() }),
            "tag {tag} bits {bits}"
        );
    }
}

// [Requirement 3: extensible with an unsupported sub-format | spec-narrate-generation | FR-009]
#[test]
fn extensible_with_unsupported_bit_depth_is_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let body = riff(&fmt_extensible(3, 1, 48_000, 64), &[0u8; 800]);
    let p = put(&dir, "ext64.wav", &body);
    assert_eq!(
        inspect_clip(&p),
        Err(ClipError::Unsupported {
            path: p.clone(),
            detail: "format tag 65534, 64 bits".to_string()
        })
    );
}

// ---------------------------------------------------------------------------------------
// inspect_clip: not found and unreadable
// ---------------------------------------------------------------------------------------

// [Scenario: Clip missing | spec-narrate-generation Req 3 | FR-009]
#[test]
fn missing_clip_is_not_found_with_the_given_path() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("gone.wav");
    assert_eq!(inspect_clip(&p), Err(ClipError::NotFound { path: p.clone() }));
}

// [Scenario: Not a WAV | spec-narrate-generation Req 3 | FR-009]
#[test]
fn text_file_and_empty_file_are_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        ("fake.wav", &b"this is not a wav file at all"[..]),
        ("empty.wav", &b""[..]),
        ("riff-only.wav", &b"RIFF"[..]),
        ("short-header.wav", &b"RIFF\x24\0\0\0WAVE"[..]),
    ] {
        let p = put(&dir, name, bytes);
        match inspect_clip(&p) {
            Err(ClipError::Unreadable { path, .. }) => assert_eq!(path, p, "{name}"),
            other => panic!("{name}: expected Unreadable, got {other:?}"),
        }
    }
}

// [Scenario: Not a WAV | spec-narrate-generation Req 3 | FR-009]
#[test]
fn riff_container_that_is_not_wave_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = pcm16_mono_24k(100);
    bytes[8..12].copy_from_slice(b"AVI ");
    let p = put(&dir, "avi.wav", &bytes);
    assert!(matches!(inspect_clip(&p), Err(ClipError::Unreadable { .. })));
}

// [Scenario: Not a WAV | spec-narrate-generation Req 3 | FR-009]
#[test]
fn data_chunk_declaring_more_bytes_than_the_file_holds_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = pcm16_mono_24k(50); // 100 data bytes present
    // Patch the data chunk size (offset 40 in a 44-byte header) to declare 48000 bytes.
    bytes[40..44].copy_from_slice(&48_000u32.to_le_bytes());
    let p = put(&dir, "truncated.wav", &bytes);
    match inspect_clip(&p) {
        Err(ClipError::Unreadable { path, .. }) => assert_eq!(path, p),
        other => panic!("expected Unreadable, got {other:?}"),
    }
}

// [Scenario: Not a WAV | spec-narrate-generation Req 3 | FR-009]
#[test]
fn riff_without_a_fmt_chunk_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    body.extend_from_slice(&chunk(b"data", &[0u8; 100]));
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&body);
    let p = put(&dir, "nofmt.wav", &bytes);
    assert!(matches!(inspect_clip(&p), Err(ClipError::Unreadable { .. })));
}

// ---------------------------------------------------------------------------------------
// is_too_short, needs_trim, duration_secs
// ---------------------------------------------------------------------------------------

// [Scenario: Exactly 5 s rejected | spec-narrate-generation Req 3 | FR-009]
#[test]
fn is_too_short_is_true_up_to_and_including_exactly_five_seconds() {
    assert!(is_too_short(&info(24_000, 119_999)));
    assert!(is_too_short(&info(24_000, 120_000)), "exactly 5.0 s is rejected");
    assert!(!is_too_short(&info(24_000, 120_001)));
    assert!(is_too_short(&info(44_100, 220_500)));
    assert!(!is_too_short(&info(44_100, 220_501)));
    assert!(is_too_short(&info(8_000, 0)));
}

// [Scenario: Exactly 5 s rejected | spec-narrate-generation Req 3 | FR-009]
#[test]
fn is_too_short_uses_integer_arithmetic_without_u32_overflow() {
    let rate = u32::MAX;
    assert!(is_too_short(&info(rate, 5 * rate as u64)));
    assert!(!is_too_short(&info(rate, 5 * rate as u64 + 1)));
}

// [Scenario: Valid long clip | spec-narrate-generation Req 3 | FR-009]
#[test]
fn needs_trim_is_true_only_above_ten_seconds() {
    assert!(!needs_trim(&info(24_000, 168_000))); // 7 s
    assert!(!needs_trim(&info(24_000, 239_999)));
    assert!(!needs_trim(&info(24_000, 240_000)), "exactly 10.0 s needs no trim");
    assert!(needs_trim(&info(24_000, 240_001)));
    assert!(needs_trim(&info(24_000, 288_000))); // 12 s
    assert!(!needs_trim(&info(48_000, 480_000)));
    assert!(needs_trim(&info(48_000, 480_001)));
    let rate = u32::MAX;
    assert!(!needs_trim(&info(rate, 10 * rate as u64)));
    assert!(needs_trim(&info(rate, 10 * rate as u64 + 1)));
}

// [Public API: duration_secs | spec-narrate-generation Req 3 | FR-009]
#[test]
fn duration_secs_is_frames_divided_by_sample_rate() {
    assert!((info(24_000, 60_000).duration_secs() - 2.5).abs() < 1e-9);
    assert!((info(24_000, 288_000).duration_secs() - 12.0).abs() < 1e-9);
    assert!((info(44_100, 22_050).duration_secs() - 0.5).abs() < 1e-9);
    assert_eq!(info(24_000, 0).duration_secs(), 0.0);
}

// ---------------------------------------------------------------------------------------
// write_clip_prefix
// ---------------------------------------------------------------------------------------

// [Scenario: Valid long clip | spec-narrate-generation Req 3 | FR-009]
#[test]
fn write_clip_prefix_writes_exactly_the_first_ten_seconds_in_the_same_format() {
    let dir = tempfile::tempdir().unwrap();
    let src_bytes = pcm16_mono_24k(288_000); // 12 s
    let src = put(&dir, "src.wav", &src_bytes);
    let dst = dir.path().join("dst.wav");
    let returned = write_clip_prefix(&src, &dst, 240_000).unwrap();
    assert_eq!(
        (returned.sample_rate, returned.channels, returned.bits_per_sample, returned.is_float),
        (24_000, 1, 16, false)
    );
    let written = inspect_clip(&dst).unwrap();
    assert_eq!(written.frames, 240_000);
    assert_eq!(
        (written.sample_rate, written.channels, written.bits_per_sample, written.is_float),
        (24_000, 1, 16, false)
    );
    assert!(!needs_trim(&written));
    assert!(!is_too_short(&written));
    let src_data = data_chunk(&src_bytes);
    let dst_data = data_chunk(&std::fs::read(&dst).unwrap());
    assert_eq!(dst_data.len(), 240_000 * 2);
    assert_eq!(dst_data, src_data[..240_000 * 2].to_vec());
}

// [Requirement 3: same format, frames not samples | spec-narrate-generation | FR-009]
#[test]
fn write_clip_prefix_counts_frames_and_keeps_channels_depth_and_float_format() {
    let dir = tempfile::tempdir().unwrap();
    // (name, tag, channels, rate, bits, expected is_float)
    let cases: &[(&str, u16, u16, u32, u16, bool)] = &[
        ("stereo16.wav", 1, 2, 44_100, 16, false),
        ("mono24.wav", 1, 1, 48_000, 24, false),
        ("mono8.wav", 1, 1, 8_000, 8, false),
        ("float32.wav", 3, 1, 48_000, 32, true),
    ];
    for (name, tag, ch, rate, bits, is_float) in cases {
        let src_bytes = wav(*tag, *ch, *rate, *bits, 1_000);
        let src = put(&dir, name, &src_bytes);
        let dst = dir.path().join(format!("prefix-{name}"));
        write_clip_prefix(&src, &dst, 100).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let written = inspect_clip(&dst).unwrap();
        assert_eq!(
            written,
            ClipInfo {
                sample_rate: *rate,
                channels: *ch,
                bits_per_sample: *bits,
                is_float: *is_float,
                frames: 100
            },
            "{name}"
        );
        let block = *ch as usize * (*bits as usize).div_ceil(8);
        let src_data = data_chunk(&src_bytes);
        let dst_data = data_chunk(&std::fs::read(&dst).unwrap());
        assert_eq!(dst_data, src_data[..100 * block].to_vec(), "{name} data prefix");
    }
}

// [Requirement 3: errors from write_clip_prefix | spec-narrate-generation | FR-009]
#[test]
fn write_clip_prefix_reports_source_errors_with_the_source_path() {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("dst.wav");

    let missing = dir.path().join("missing.wav");
    assert_eq!(
        write_clip_prefix(&missing, &dst, 100).map(|_| ()),
        Err(ClipError::NotFound { path: missing.clone() })
    );

    let garbage = put(&dir, "garbage.wav", b"not a wav");
    match write_clip_prefix(&garbage, &dst, 100) {
        Err(ClipError::Unreadable { path, .. }) => assert_eq!(path, garbage),
        other => panic!("expected Unreadable, got {other:?}"),
    }

    let alaw = put(&dir, "alaw.wav", &wav(6, 1, 8_000, 8, 1_000));
    assert_eq!(
        write_clip_prefix(&alaw, &dst, 100).map(|_| ()),
        Err(ClipError::Unsupported {
            path: alaw.clone(),
            detail: "format tag 6, 8 bits".to_string()
        })
    );
}

// ---------------------------------------------------------------------------------------
// Type-level contract
// ---------------------------------------------------------------------------------------

// [Public API: derives | spec-narrate-generation Req 3 | FR-009]
#[test]
fn clip_types_implement_the_documented_traits() {
    assert_is_error::<ClipError>();
    let a = info(24_000, 10);
    let b = a; // Copy
    assert_eq!(a, b);
    let e = ClipError::NotFound { path: Path::new("x.wav").to_path_buf() };
    assert_eq!(e.clone(), e);
    assert!(!e.to_string().is_empty());
}

// [Public API re-exports | spec-narrate-generation Req 3 | FR-009]
#[test]
fn clip_items_are_re_exported_at_the_crate_root() {
    let i: narrate_core::ClipInfo = info(24_000, 120_000);
    assert!(narrate_core::is_too_short(&i));
    assert!(!narrate_core::needs_trim(&i));
    let _: fn(&Path) -> Result<narrate_core::ClipInfo, narrate_core::ClipError> =
        narrate_core::inspect_clip;
}
