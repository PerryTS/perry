//! Byte-exact compressed payload registration before JavaScript/GC startup.
//! Only the Bun utility pack links zstd; ordinary embedding remains unchanged.

use super::register_asset;

fn decode(packed: &[u8], original_len: usize) -> Option<Box<[u8]>> {
    if original_len > isize::MAX as usize
        || zstd::zstd_safe::get_frame_content_size(packed).ok()?? != original_len as u64
        || zstd::zstd_safe::find_frame_compressed_size(packed).ok()? != packed.len()
    {
        return None;
    }
    // Validate the frame's own length before reserving. Never let an unchecked
    // caller length drive an allocation, or accept trailing/concatenated data.
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(original_len).ok()?;
    let written = zstd::bulk::Decompressor::new()
        .ok()?
        .decompress_to_buffer(packed, &mut bytes)
        .ok()?;
    if written != original_len || bytes.len() != original_len {
        return None;
    }
    Some(bytes.into_boxed_slice())
}

/// Register one compiler-emitted zstd frame, returning 1 on success and 0 on
/// invalid metadata, corrupt/truncated input, or a failed allocation/decode.
/// No entry is published on failure. The generated constructor treats 0 as
/// fatal rather than exposing compressed bytes as a file or silently omitting it.
///
/// Names are copied by the existing registry; decoded native storage is kept
/// for the process lifetime, exactly like the raw API's immortal byte range.
/// This does not allocate JavaScript values or access an initialized GC.
///
/// # Safety
/// Non-null pointers must name readable ranges of the supplied lengths for
/// this call. Compressed input need not outlive the call. `text_module` must be
/// 0 (ordinary file) or 1 (explicit text loader); other values are rejected.
#[no_mangle]
pub unsafe extern "C" fn js_register_embedded_zstd_asset(
    name_ptr: *const u8,
    name_len: usize,
    packed_ptr: *const u8,
    packed_len: usize,
    original_len: usize,
    text_module: u32,
) -> i32 {
    if name_ptr.is_null()
        || packed_ptr.is_null()
        || name_len > isize::MAX as usize
        || packed_len > isize::MAX as usize
        || text_module > 1
    {
        return 0;
    }
    let packed = std::slice::from_raw_parts(packed_ptr, packed_len);
    let Some(decoded) = decode(packed, original_len) else {
        return 0;
    };
    let bytes = Box::leak(decoded);
    register_asset(
        name_ptr,
        name_len,
        bytes.as_ptr(),
        bytes.len(),
        text_module == 1,
    );
    1
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_REGISTER_ZSTD: unsafe extern "C" fn(
    *const u8,
    usize,
    *const u8,
    usize,
    usize,
    u32,
) -> i32 = js_register_embedded_zstd_asset;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_bytes_and_lengths_including_empty_and_non_utf8() {
        for bytes in [
            &b""[..],
            &b"plain\0text\xff\xc3\xa9"[..],
            &vec![b'x'; 32768][..],
        ] {
            let packed = zstd::bulk::compress(bytes, 3).unwrap();
            assert_eq!(decode(&packed, bytes.len()).unwrap().as_ref(), bytes);
            assert!(decode(&packed, bytes.len() + 1).is_none());
            if !bytes.is_empty() {
                assert!(decode(&packed, bytes.len() - 1).is_none());
            }
        }
    }

    #[test]
    fn malformed_truncated_trailing_and_concatenated_frames_are_rejected() {
        let packed = zstd::bulk::compress(&vec![b'x'; 32768], 3).unwrap();
        for end in 0..packed.len() {
            assert!(decode(&packed[..end], 32768).is_none(), "prefix {end}");
        }
        let mut invalid = packed.clone();
        invalid[0] ^= 0xff;
        assert!(decode(&invalid, 32768).is_none());
        let mut trailing = packed.clone();
        trailing.push(0);
        assert!(decode(&trailing, 32768).is_none());
        let mut concatenated = packed.clone();
        concatenated.extend(zstd::bulk::compress(b"", 3).unwrap());
        assert!(decode(&concatenated, 32768).is_none());
        assert!(decode(&packed, usize::MAX).is_none());
    }

    #[test]
    fn payload_checksum_damage_is_rejected() {
        let mut compressor = zstd::bulk::Compressor::new(3).unwrap();
        compressor.include_checksum(true).unwrap();
        let bytes = vec![b'x'; 32768];
        let mut packed = compressor.compress(&bytes).unwrap();
        assert_eq!(decode(&packed, bytes.len()).unwrap().as_ref(), bytes);
        *packed.last_mut().unwrap() ^= 1;
        assert!(decode(&packed, bytes.len()).is_none());
    }

    #[test]
    fn registry_preserves_loader_path_size_and_ownership() {
        let bytes = b"retained\0bytes\xff";
        let mut packed = zstd::bulk::compress(bytes, 3).unwrap();
        unsafe {
            for (name, text) in [
                ("compressed-test/file.bin", 0),
                ("compressed-test/text.md", 1),
            ] {
                assert_eq!(
                    js_register_embedded_zstd_asset(
                        name.as_ptr(),
                        name.len(),
                        packed.as_ptr(),
                        packed.len(),
                        bytes.len(),
                        text
                    ),
                    1
                );
            }
        }
        packed.fill(0); // Registry must not retain any borrow of the input.
        drop(packed);
        assert_eq!(
            super::super::lookup("$perryfs/compressed-test/file.bin"),
            Some(bytes.as_slice())
        );
        assert!(super::super::lookup_text_module("compressed-test/file.bin").is_none());
        assert_eq!(
            super::super::lookup_text_module("compressed-test/text.md"),
            Some(bytes.as_slice())
        );
        assert_eq!(
            super::super::metadata("compressed-test/file.bin")
                .unwrap()
                .size,
            bytes.len()
        );
    }

    #[test]
    fn invalid_registration_does_not_publish_a_partial_entry() {
        let name = b"compressed-test/invalid";
        let packed = zstd::bulk::compress(b"value", 3).unwrap();
        unsafe {
            assert_eq!(
                js_register_embedded_zstd_asset(
                    name.as_ptr(),
                    name.len(),
                    packed.as_ptr(),
                    packed.len(),
                    5,
                    2
                ),
                0
            );
            assert_eq!(
                js_register_embedded_zstd_asset(
                    name.as_ptr(),
                    name.len(),
                    packed.as_ptr(),
                    packed.len() - 1,
                    5,
                    0
                ),
                0
            );
            assert_eq!(
                js_register_embedded_zstd_asset(
                    name.as_ptr(),
                    name.len(),
                    std::ptr::null(),
                    0,
                    5,
                    0
                ),
                0
            );
        }
        assert!(super::super::lookup("compressed-test/invalid").is_none());
    }
}
