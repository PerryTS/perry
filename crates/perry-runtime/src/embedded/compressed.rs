//! Byte-exact compressed payload registration before JavaScript/GC startup.
//! Only the Bun utility pack links zstd; ordinary embedding remains unchanged.

use super::{push_asset, register_asset, AssetBytes};

/// The frame declares exactly `original_len` content bytes and occupies all of
/// `packed` (no truncation, trailing or concatenated data). Reads frame and
/// block headers only; the payload checksum is verified by [`decode`].
fn frame_matches(packed: &[u8], original_len: usize) -> Option<()> {
    (original_len <= isize::MAX as usize
        && zstd::zstd_safe::get_frame_content_size(packed).ok()?? == original_len as u64
        && zstd::zstd_safe::find_frame_compressed_size(packed).ok()? == packed.len())
    .then_some(())
}

pub(super) fn decode(packed: &[u8], original_len: usize) -> Option<Box<[u8]>> {
    frame_matches(packed, original_len)?;
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

/// Register one compiler-emitted zstd frame that lives in immortal read-only
/// data, deferring the decode to the first read of the asset. Frame metadata
/// is still validated here, so truncated, trailing or mis-sized frames are
/// rejected (0) before `main` exactly like the eager form; checksum damage is
/// reported fatally on first read instead. Size queries and directory listings
/// never decode. Returns 1 on success.
///
/// # Safety
/// Non-null pointers must name readable ranges of the supplied lengths; the
/// compressed range must stay valid for the life of the process (the compiler
/// emits it into `.rodata`). `text_module` must be 0 or 1.
#[no_mangle]
pub unsafe extern "C" fn js_register_embedded_zstd_asset_lazy(
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
    let packed: &'static [u8] = std::slice::from_raw_parts(packed_ptr, packed_len);
    if frame_matches(packed, original_len).is_none() {
        return 0;
    }
    let name = String::from_utf8_lossy(std::slice::from_raw_parts(name_ptr, name_len)).into_owned();
    push_asset(
        name,
        AssetBytes::Compressed {
            packed,
            original_len,
        },
        text_module == 1,
    );
    1
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REGISTER_ZSTD_LAZY: unsafe extern "C" fn(
    *const u8,
    usize,
    *const u8,
    usize,
    usize,
    u32,
) -> i32 = js_register_embedded_zstd_asset_lazy;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
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

    #[test]
    fn lazy_registration_reports_size_without_decoding_and_reads_exact_bytes() {
        let bytes = b"lazy\0payload\xff".repeat(4096);
        let mut compressor = zstd::bulk::Compressor::new(3).unwrap();
        compressor.include_checksum(true).unwrap();
        let packed: &'static [u8] =
            Box::leak(compressor.compress(&bytes).unwrap().into_boxed_slice());
        // Checksum damage passes the header-only registration check. Its size
        // and presence are answered without decoding; only a read would fail.
        let mut damaged = packed.to_vec();
        *damaged.last_mut().unwrap() ^= 1;
        let damaged: &'static [u8] = Box::leak(damaged.into_boxed_slice());
        unsafe {
            for (name, frame, text) in [
                ("lazy-test/file.bin", packed, 0),
                ("lazy-test/text.md", packed, 1),
                ("lazy-test/damaged.bin", damaged, 0),
            ] {
                assert_eq!(
                    js_register_embedded_zstd_asset_lazy(
                        name.as_ptr(),
                        name.len(),
                        frame.as_ptr(),
                        frame.len(),
                        bytes.len(),
                        text
                    ),
                    1
                );
            }
        }
        assert_eq!(
            super::super::metadata("lazy-test/damaged.bin")
                .unwrap()
                .size,
            bytes.len()
        );
        assert_eq!(
            super::super::lookup("$perryfs/lazy-test/file.bin"),
            Some(bytes.as_slice())
        );
        // A second read serves the cached decode.
        let first = super::super::lookup("lazy-test/file.bin").unwrap();
        assert_eq!(
            super::super::lookup("lazy-test/file.bin").unwrap().as_ptr(),
            first.as_ptr()
        );
        assert!(super::super::lookup_text_module("lazy-test/file.bin").is_none());
        assert_eq!(
            super::super::lookup_text_module("lazy-test/text.md"),
            Some(bytes.as_slice())
        );
    }

    #[test]
    fn lazy_registration_rejects_mis_sized_and_truncated_frames() {
        let name = b"lazy-test/invalid";
        let packed: &'static [u8] = Box::leak(
            zstd::bulk::compress(b"value", 3)
                .unwrap()
                .into_boxed_slice(),
        );
        unsafe {
            for (len, original, text) in [
                (packed.len(), 6, 0),
                (packed.len() - 1, 5, 0),
                (packed.len(), 5, 2),
            ] {
                assert_eq!(
                    js_register_embedded_zstd_asset_lazy(
                        name.as_ptr(),
                        name.len(),
                        packed.as_ptr(),
                        len,
                        original,
                        text
                    ),
                    0
                );
            }
        }
        assert!(super::super::metadata("lazy-test/invalid").is_none());
    }
}
