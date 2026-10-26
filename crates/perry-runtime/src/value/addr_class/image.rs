//! Immutable AOT strings live in one linker-owned section. Bounds are fixed
//! for the loaded image; no string address is registered or cached.
use super::*;

// Always create the section, including in Rust tests and runtime-only links.
// The sentinel is not a string and cannot pass the header checks below.
// On wasm it is what makes the linker synthesize `__start_perry_strings` and
// `__stop_perry_strings` even for a program with no string literal.
// `used(linker)` keeps it through `-dead_strip` / `--gc-sections` / `/OPT:REF`,
// and `bounds()` references it, so the object that needs the section bounds
// always brings the section with it. Without both, ld64 drops the section in a
// program that has no image strings and then cannot synthesize
// `section$start/end$__TEXT$__perry_strings`, so every link fails.
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_SECTION_SENTINEL: [u64; 1] = [0];

#[cfg(target_os = "windows")]
#[used(linker)]
#[link_section = ".perrystr$a"]
static IMAGE_START: [u64; 1] = [0];
#[cfg(target_os = "windows")]
#[used(linker)]
#[link_section = ".perrystr$z"]
static IMAGE_END: [u64; 1] = [0];

#[inline]
pub(super) fn bounds() -> (usize, usize) {
    // A real reference: whatever links this function links the sentinel too.
    std::hint::black_box(std::ptr::addr_of!(IMAGE_SECTION_SENTINEL));
    #[cfg(target_vendor = "apple")]
    {
        // ld64 synthesizes `section$start$SEG$SECT` only under that exact
        // name. The leading \x01 tells LLVM not to add Mach-O's `_` global
        // prefix; without it the linker sees `_section$start$...`, which it
        // never defines, and every link fails.
        unsafe extern "C" {
            #[link_name = "\u{1}section$start$__TEXT$__perry_strings"]
            static START: u8;
            #[link_name = "\u{1}section$end$__TEXT$__perry_strings"]
            static END: u8;
        }
        (
            std::ptr::addr_of!(START) as usize,
            std::ptr::addr_of!(END) as usize,
        )
    }
    #[cfg(target_os = "windows")]
    {
        (
            std::ptr::addr_of!(IMAGE_START) as usize + 8,
            std::ptr::addr_of!(IMAGE_END) as usize,
        )
    }
    // ELF and wasm-ld both synthesize these for the C-identifier section name.
    #[cfg(not(any(target_vendor = "apple", target_os = "windows")))]
    {
        unsafe extern "C" {
            static __start_perry_strings: u8;
            static __stop_perry_strings: u8;
        }
        (
            std::ptr::addr_of!(__start_perry_strings) as usize,
            std::ptr::addr_of!(__stop_perry_strings) as usize,
        )
    }
}

/// Validate every byte range BEFORE reading it. An aligned forged pointer
/// inside the section may name padding or string data, so bounds alone do not
/// certify a header. Its marker, layout, length and complete payload must agree.
#[cold]
#[inline(never)]
pub(super) unsafe fn header(addr: usize) -> Option<std::ptr::NonNull<GcHeader>> {
    use crate::string::StringHeader;
    let header_addr = addr.checked_sub(GC_HEADER_SIZE)?;
    let (start, end) = bounds();
    // Runtime images are not in the section on wasm; they own exact ranges
    // registered at startup. Program literals are in it, so the section
    // bounds are exact for them as on every other target.
    #[cfg(target_family = "wasm")]
    let (start, end) = if (start..end).contains(&header_addr) {
        (start, end)
    } else {
        crate::wasm_image::bounds(header_addr)?
    };
    let data = addr.checked_add(std::mem::size_of::<StringHeader>())?;
    if header_addr < start || data >= end || addr % 8 != 0 {
        return None;
    }
    let gc = &*(header_addr as *const GcHeader);
    if gc.obj_type != crate::gc::GC_TYPE_STRING
        || gc._reserved != crate::string::STRING_IMAGE_MARKER
        || gc.gc_flags
            != (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_PINNED | crate::gc::GC_FLAG_INTERNED)
    {
        return None;
    }
    let string = &*(addr as *const StringHeader);
    let payload_end = data.checked_add(string.byte_len as usize)?.checked_add(1)?;
    // The image extends past the NUL to its 8-aligned atom hash; the whole
    // extent, not just the payload, must lie in the section.
    let image_end = header_addr.checked_add(gc.size as usize)?;
    if image_end > end
        || string.byte_len != string.capacity
        || string.utf16_len > string.byte_len
        || gc.size as usize != crate::string::image_string_gc_size(string.byte_len)
        || *(payload_end as *const u8).sub(1) != 0
    {
        return None;
    }
    std::ptr::NonNull::new(header_addr as *mut GcHeader)
}
