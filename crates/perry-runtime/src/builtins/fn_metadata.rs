//! Names and retained source are owned by a function's descriptor entry.
//! Closure reads index their owner/ordinal; only stack and raw method code
//! locations search the canonical image descriptors.
use super::fn_metadata_owner::Record;
#[cfg(test)]
use crate::codegen_abi::LayoutWord;

/// Prove ASCII with covering loads that stay wholly inside the slice.
/// Overlap only repeats bytes in the OR; it cannot change their high bits.
#[inline(always)]
fn registered_bytes_are_ascii(bytes: &[u8]) -> bool {
    let len = bytes.len();
    let ptr = bytes.as_ptr();
    const WORD: usize = std::mem::size_of::<usize>();
    const HIGH_BITS: usize = usize::MAX / 255 * 128;
    // SAFETY: each width is admitted by len before either covering load.
    // The loop and final suffix also end at or before len. No alignment is
    // required, and OR makes endianness and overlap irrelevant to the proof.
    unsafe {
        if len >= WORD {
            let end = len - WORD;
            let mut bits = ptr.cast::<usize>().read_unaligned();
            let mut offset = WORD;
            while offset < end {
                bits |= ptr.add(offset).cast::<usize>().read_unaligned();
                offset += WORD;
            }
            bits |= ptr.add(end).cast::<usize>().read_unaligned();
            bits & HIGH_BITS == 0
        } else if len >= std::mem::size_of::<u32>() {
            let bits = ptr.cast::<u32>().read_unaligned()
                | ptr.add(len - 4).cast::<u32>().read_unaligned();
            bits & 0x8080_8080 == 0
        } else if len >= std::mem::size_of::<u16>() {
            let bits = ptr.cast::<u16>().read_unaligned()
                | ptr.add(len - 2).cast::<u16>().read_unaligned();
            bits & 0x8080 == 0
        } else {
            len == 0 || *ptr < 0x80
        }
    }
}

#[inline]
pub(super) fn decode_registered_bytes(bytes: &[u8]) -> Option<&str> {
    if registered_bytes_are_ascii(bytes) {
        // SAFETY: every byte is below 0x80, including embedded NULs.
        Some(unsafe { std::str::from_utf8_unchecked(bytes) })
    } else {
        std::str::from_utf8(bytes).ok()
    }
}
fn descriptor_pointer<T>(base: *const u8, offset: i32) -> *const T {
    (base as usize).wrapping_add_signed(offset as isize) as *const T
}
pub(super) fn info_name_bytes(info: &crate::closure::JsFunctionInfo) -> Option<&[u8]> {
    if info.flags & crate::codegen_abi::FN_HAS_NAME == 0 {
        return None;
    }
    unsafe {
        let base = (info as *const crate::closure::JsFunctionInfo).cast::<u8>();
        let tail = base
            .add(std::mem::size_of_val(info) + crate::codegen_abi::function_source_tail_size(info));
        let delta = tail.cast::<i32>().read_unaligned();
        let len = tail.add(4).cast::<u32>().read_unaligned() as usize;
        Some(std::slice::from_raw_parts(
            descriptor_pointer(base, delta),
            len,
        ))
    }
}
#[cfg(test)]
#[path = "fn_metadata_utf8_tests.rs"]
mod utf8_read_tests;
fn with_registered_name<R>(
    info: &crate::closure::JsFunctionInfo,
    read: impl FnOnce(&str) -> R,
) -> Option<R> {
    if let Some(record) = Record::for_info(info) {
        record.with_name(read)
    } else {
        info_name_bytes(info)
            .and_then(decode_registered_bytes)
            .map(read)
    }
}
pub(crate) fn registered_name_string(info: &crate::closure::JsFunctionInfo) -> Option<String> {
    with_registered_name(info, str::to_owned)
}
/// Copy a name into its canonical descriptor entry and report whether
/// publication succeeded, so an immediate consumer need not read it back. This
/// says nothing about UTF-8; reads keep their validation and image-name
/// precedence. Input need only outlive this call. No code-pointer token or
/// address routing is accepted.
pub(crate) fn register_function_name(info: &crate::closure::JsFunctionInfo, bytes: &[u8]) -> bool {
    Record::for_info(info).is_some_and(|record| record.write_name(bytes, false))
}
pub(crate) fn register_function_name_if_absent(info: &crate::closure::JsFunctionInfo, name: &str) {
    if let Some(record) = Record::for_info(info) {
        record.write_name(name.as_bytes(), true);
    }
}
pub(crate) fn function_is_non_strict_ordinary_for_info(
    info: &crate::closure::JsFunctionInfo,
) -> bool {
    Record::for_info(info)
        .map(|r| r.ordinary(info.flags & crate::codegen_abi::FN_NON_STRICT_ORDINARY != 0))
        .unwrap_or(info.flags & crate::codegen_abi::FN_NON_STRICT_ORDINARY != 0)
}
/// The image string a compiled body's name range is the text of
/// ([`FN_NAME_IMAGE`](crate::codegen_abi::FN_NAME_IMAGE)): the function's own
/// `name` value, static data of its body, with nothing to allocate. `None`
/// for a body whose name is not image data.
///
/// # Safety
/// `info` is null or a live function info.
#[inline(always)]
pub(crate) unsafe fn function_name_image(
    info: *const crate::closure::JsFunctionInfo,
) -> Option<*const crate::StringHeader> {
    let body = info.as_ref()?;
    const IMAGE_NAME: u32 = crate::codegen_abi::FN_HAS_NAME | crate::codegen_abi::FN_NAME_IMAGE;
    if body.flags & IMAGE_NAME != IMAGE_NAME {
        return None;
    }
    let base = info.cast::<u8>();
    let source_tail = crate::codegen_abi::function_source_tail_size(body);
    let tail = base.add(std::mem::size_of::<crate::closure::JsFunctionInfo>() + source_tail);
    let bytes: *const u8 = descriptor_pointer(base, tail.cast::<i32>().read_unaligned());
    Some(bytes.sub(std::mem::size_of::<crate::StringHeader>()).cast())
}
const _: () = assert!(
    std::mem::size_of::<crate::StringHeader>() == 20,
    "codegen places an image name's bytes 20 bytes past its header"
);

/// A compiled closure's immutable name range comes from its body descriptor.
/// Runtime-native names come from the explicit owner carried by their info.
///
/// # Safety
/// The closure is live; its static info owns the optional ranges its flags license.
#[inline(never)]
pub(crate) unsafe fn function_name_for_closure(
    closure: *const crate::closure::ClosureHeader,
) -> Option<String> {
    with_function_name_for_closure(closure, str::to_owned)
}

/// The former registered-name consumers retain owned-before-borrowed
/// precedence; intrinsic `.name` readers below retain info-name precedence.
/// Both use the closure's explicit metadata owner, never its code as a key.
///
/// # Safety
/// `closure` is null or live and its info is valid.
pub(crate) unsafe fn registered_name_for_closure(
    closure: *const crate::closure::ClosureHeader,
) -> Option<String> {
    let info = closure.as_ref()?.info.as_ref()?;
    if info.code.is_null() {
        return None;
    }
    registered_name_string(info).filter(|name| !name.is_empty())
}

/// Share image-range and registered-name precedence with reflection. The
/// reader runs while any metadata owner is borrowed and must not register.
unsafe fn with_function_name_for_closure<R>(
    closure: *const crate::closure::ClosureHeader,
    read: impl FnOnce(&str) -> R,
) -> Option<R> {
    let closure = closure.as_ref()?;
    let info = closure.info.as_ref()?;
    if info.flags & crate::codegen_abi::FN_HAS_NAME != 0 {
        let base = closure.info.cast::<u8>();
        let source_tail = crate::codegen_abi::function_source_tail_size(info);
        let tail = base.add(std::mem::size_of::<crate::closure::JsFunctionInfo>() + source_tail);
        let delta = tail.cast::<i32>().read_unaligned();
        let len = tail.add(4).cast::<u32>().read_unaligned() as usize;
        // Compiler-owned names originate as UTF-8; empty image names are
        // valid too, exactly as in the String-returning reflection reader.
        let bytes = std::slice::from_raw_parts(descriptor_pointer(base, delta), len);
        return Some(read(std::str::from_utf8_unchecked(bytes)));
    }
    with_registered_name(info, |name| (!name.is_empty()).then(|| read(name))).flatten()
}

/// Non-allocating presence with the same UTF-8 and override precedence as
/// registered-name reflection.
pub(crate) fn function_has_registered_name(info: &crate::closure::JsFunctionInfo) -> bool {
    with_registered_name(info, |name| !name.is_empty()).unwrap_or(false)
}

/// # Safety
/// `closure` is null or live; its info owns the ranges licensed by its flags.
pub(crate) unsafe fn function_name_matches_closure(
    closure: *const crate::closure::ClosureHeader,
    expected: &[u8],
) -> bool {
    with_function_name_for_closure(closure, |name| name.as_bytes() == expected).unwrap_or(false)
}

/// Read retained source from a compiled body's info record.
#[inline(always)]
pub(super) fn retained_source_for_info(info: &crate::closure::JsFunctionInfo) -> Option<String> {
    if info.flags & crate::codegen_abi::FN_HAS_SOURCE == 0 {
        return None;
    }
    crate::source_read_count::note(crate::source_read_count::ReadKind::Info);
    // Most infos pack the range into the otherwise idle versioned-clone mask.
    // Only a body that also has a versioned clone uses the optional tail.
    unsafe {
        let base = info as *const crate::closure::JsFunctionInfo as *const u8;
        if info.flags & crate::codegen_abi::FN_SOURCE_BLOCKS != 0 {
            crate::source_read_count::note(crate::source_read_count::ReadKind::Framed);
            let (record, byte_len) = if info.versioned_code.is_null() {
                (
                    std::ptr::addr_of!(info.versioned_boxed_mask).cast::<u8>(),
                    info.versioned_captures as usize,
                )
            } else {
                (
                    base.add(std::mem::size_of_val(info)),
                    (base.add(std::mem::size_of_val(info) + 8).cast::<u32>()).read_unaligned()
                        as usize,
                )
            };
            let displacement = record.cast::<i32>().read_unaligned() as isize;
            let offset = record.add(4).cast::<u32>().read_unaligned() as usize;
            if byte_len == 0 {
                return None;
            }
            let bytes =
                super::retained_source::decode_range(base.offset(displacement), offset, byte_len);
            // Source ranges originated as &str; block boundaries never become
            // str boundaries, and no lossy conversion touches their bytes.
            return Some(String::from_utf8_unchecked(bytes));
        }
        let (displacement, byte_len) = if info.versioned_code.is_null() {
            (
                info.versioned_boxed_mask as i64 as isize,
                info.versioned_captures as usize,
            )
        } else {
            // SAFETY: with FN_HAS_SOURCE and a live versioned mask, codegen
            // emits `{ JsFunctionInfo, i32 displacement, u32 length }`.
            let tail = base.add(crate::codegen_abi::JS_FUNCTION_INFO_SIZE);
            (
                (tail as *const i32).read_unaligned() as isize,
                (tail.add(std::mem::size_of::<i32>()) as *const u32).read_unaligned() as usize,
            )
        };
        if byte_len == 0 {
            return None;
        }
        let bytes = std::slice::from_raw_parts(base.offset(displacement), byte_len);
        crate::source_read_log::note("info", bytes);
        // Compiler source enters this record as Rust `&str`, so it is valid
        // UTF-8 by construction. Avoid the registry's defensive lossy decode
        // on this compact, compiler-owned path.
        Some(String::from_utf8_unchecked(bytes.to_vec()))
    }
}

pub fn function_source_for_info_or_native(info: &crate::closure::JsFunctionInfo) -> String {
    // Immutable info ranges retain their established precedence over explicit
    // registration; registered reads separately use owned-before-borrowed.
    if let Some(source) = retained_source_for_info(info) {
        return source;
    }
    if let Some(source) = Record::for_info(info).and_then(|r| r.source()) {
        return source;
    }
    let name = registered_name_string(info).unwrap_or_default();
    format!("function {name}() {{ [native code] }}")
}
pub fn function_source_for_func_ptr(code: usize) -> String {
    if let Some(source) = super::fn_metadata_owner::source_for_code(code) {
        return source;
    }
    let name = super::fn_metadata_owner::name_for_code(code).unwrap_or_default();
    format!("function {name}() {{ [native code] }}")
}
#[cfg(test)]
mod descriptor_name_tests {
    use super::*;
    #[repr(C)]
    struct Image {
        info: crate::closure::JsFunctionInfo,
        tail: [i32; 6],
        name: [u8; 5],
    }
    extern "C" fn body(_: *const crate::closure::ClosureHeader, _: crate::closure::JsThis) -> f64 {
        0.0
    }
    #[test]
    fn name_range_is_read_after_the_optional_source_range() {
        for source_tail in [0usize, 8, 16] {
            let mut image = Image {
                info: crate::closure::JsFunctionInfo::of(
                    body as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
                ),
                tail: [0; 6],
                name: *b"named",
            };
            image.info.flags |= crate::codegen_abi::FN_HAS_NAME;
            if source_tail != 0 {
                image.info.flags |= crate::codegen_abi::FN_HAS_SOURCE;
                if source_tail == 16 {
                    image.info.flags |= crate::codegen_abi::FN_SOURCE_BLOCKS;
                }
                image.info.versioned_code = body as *const u8;
            }
            let slot = source_tail / 4;
            image.tail[slot] =
                (image.name.as_ptr() as isize - &image.info as *const _ as isize) as i32;
            image.tail[slot + 1] = 5;
            let closure = crate::closure::ClosureHeader {
                capture_count: 0,
                shape_id: LayoutWord(crate::codegen_abi::FUNCTION_LAYOUT_WORD),
                info: std::ptr::addr_of!(image.info),
                props: std::ptr::null_mut(),
            };
            assert_eq!(
                unsafe { function_name_for_closure(&closure) }.as_deref(),
                Some("named")
            );
            assert!(unsafe { function_name_matches_closure(&closure, b"named") });
            assert!(!unsafe { function_name_matches_closure(&closure, b"other") });
        }
    }
}

#[cfg(test)]
mod compressed_source_tests {
    use super::*;
    #[repr(C)]
    struct Image {
        info: crate::closure::JsFunctionInfo,
        tail: [u32; 4],
        frame: [u8; 128],
    }
    extern "C" fn body(_: *const crate::closure::ClosureHeader, _: crate::closure::JsThis) -> f64 {
        0.0
    }
    #[test]
    fn info_reads_the_same_nested_utf8_range_with_and_without_a_clone() {
        let source = "function nested() { /*世界😀*/ return '\\ud800'; }";
        let raw = format!("xx{source}tail");
        for versioned in [false, true] {
            let mut image = Image {
                info: crate::closure::JsFunctionInfo::of(
                    body as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
                ),
                tail: [0; 4],
                frame: [0; 128],
            };
            image.info.flags |=
                crate::codegen_abi::FN_HAS_SOURCE | crate::codegen_abi::FN_SOURCE_BLOCKS;
            let len = raw.len() as u16;
            image.frame[..4].copy_from_slice(&(u32::from(len) + 5).to_le_bytes());
            image.frame[4..8].copy_from_slice(&u32::from(len).to_le_bytes());
            image.frame[8] = 1;
            image.frame[9..11].copy_from_slice(&len.to_le_bytes());
            image.frame[11..13].copy_from_slice(&(!len).to_le_bytes());
            image.frame[13..13 + raw.len()].copy_from_slice(raw.as_bytes());
            let delta = (image.frame.as_ptr() as isize - &image.info as *const _ as isize) as i32;
            if versioned {
                image.info.versioned_code = body as *const u8;
                image.tail = [delta as u32, 2, source.len() as u32, 0];
            } else {
                let mut words = [0; 8];
                words[..4].copy_from_slice(&delta.to_ne_bytes());
                words[4..].copy_from_slice(&2u32.to_ne_bytes());
                image.info.versioned_boxed_mask = u64::from_ne_bytes(words);
                image.info.versioned_captures = source.len() as u32;
            }
            assert_eq!(
                retained_source_for_info(&image.info).as_deref(),
                Some(source)
            );
            assert_eq!(
                crate::codegen_abi::function_source_tail_size(&image.info),
                if versioned { 16 } else { 0 }
            );
        }
    }
}
