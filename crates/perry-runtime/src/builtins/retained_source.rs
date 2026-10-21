//! Direct source frames: no decoded source, decoder or lookup index survives a call.
use miniz_oxide::inflate::{
    core::{
        decompress, inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF, DecompressorOxide,
    },
    TINFLStatus,
};

/// Decode a compiler-owned source range. The first frame and every following
/// header/payload must be readable for the range, with little-endian u32 lengths.
/// Frames are adjacent; infos directly name the first frame and decoded offset.
pub(super) unsafe fn decode_range(
    mut frame: *const u8,
    mut offset: usize,
    mut len: usize,
) -> Vec<u8> {
    let mut result = Vec::with_capacity(len);
    let mut block = [0u8; crate::codegen_abi::RETAINED_SOURCE_BLOCK_BYTES];
    while len != 0 {
        let compressed = u32::from_le(frame.cast::<u32>().read_unaligned()) as usize;
        let decoded = u32::from_le(frame.add(4).cast::<u32>().read_unaligned()) as usize;
        assert!(
            decoded > 0 && decoded <= block.len() && offset < decoded,
            "invalid retained source frame"
        );
        assert!(
            compressed <= block.len() + 256,
            "invalid retained source frame size"
        );
        let input = std::slice::from_raw_parts(frame.add(8), compressed);
        let mut state = DecompressorOxide::new();
        let (status, consumed, written) = decompress(
            &mut state,
            input,
            &mut block[..decoded],
            0,
            TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
        );
        assert_eq!(
            (status, consumed, written),
            (TINFLStatus::Done, compressed, decoded),
            "invalid retained source DEFLATE bytes"
        );
        let take = len.min(decoded - offset);
        result.extend_from_slice(&block[offset..offset + take]);
        len -= take;
        offset = 0;
        frame = frame.add(8 + compressed);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // A stored DEFLATE block is deliberately independent of the compiler's
    // encoder: semantic bytes, including WTF-8 and a split UTF-8 codepoint.
    fn frame(bytes: &[u8]) -> Vec<u8> {
        let n = u16::try_from(bytes.len()).unwrap();
        let mut payload = vec![1];
        payload.extend_from_slice(&n.to_le_bytes());
        payload.extend_from_slice(&(!n).to_le_bytes());
        payload.extend_from_slice(bytes);
        let mut out = Vec::new();
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }

    #[test]
    fn ranges_preserve_nested_unicode_nul_and_wtf8_across_frames() {
        let bytes =
            b"prefix function nested() { return '\xe4\xb8\x96\xe7\x95\x8c\0\xed\xa0\x80'; } suffix";
        let split = 36;
        let mut encoded = frame(&bytes[..split]);
        encoded.extend_from_slice(&frame(&bytes[split..]));
        for (offset, len) in [
            (0, bytes.len()),
            (7, bytes.len() - 14),
            (split - 1, 4),
            (3, 0),
        ] {
            let got = unsafe { decode_range(encoded.as_ptr(), offset, len) };
            assert_eq!(got, bytes[offset..offset + len]);
        }
    }
}
