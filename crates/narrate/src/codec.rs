//! Decoding of `pcm_f32le_hex` audio (spec-narrate-worker.md, Protocol schemas).

/// Decode lowercase hex into little-endian float32 samples; the text must hold exactly
/// `expected` samples (8 hex digits each).
pub fn decode_f32_hex(hex: &str, expected: u64) -> Result<Vec<f32>, String> {
    let expected_len = usize::try_from(expected)
        .ok()
        .and_then(|n| n.checked_mul(8))
        .ok_or_else(|| "the sample count is too large".to_owned())?;
    if hex.len() != expected_len {
        return Err(format!(
            "expected {expected_len} hex digits for {expected} samples, got {}",
            hex.len()
        ));
    }
    let (groups, _) = hex.as_bytes().as_chunks::<8>();
    let mut samples = Vec::with_capacity(groups.len());
    for group in groups {
        let (pairs, _) = group.as_chunks::<2>();
        let mut bytes = [0u8; 4];
        for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
            *byte = (nibble(*high)? << 4) | nibble(*low)?;
        }
        samples.push(f32::from_le_bytes(bytes));
    }
    Ok(samples)
}

fn nibble(digit: u8) -> Result<u8, String> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        _ => Err("the audio data is not lowercase hex".to_owned()),
    }
}
