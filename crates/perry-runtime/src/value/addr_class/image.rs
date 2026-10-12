//! Immutable AOT strings live in one linker-owned section. Bounds are fixed
//! for the loaded image; no string address is registered or cached.
use super::*;

// Always create the section, including in Rust tests and runtime-only links.
// The sentinel is not a string and cannot pass the header checks below.
#[used]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_SECTION_SENTINEL: [u64; 1] = [0];

#[cfg(target_os = "windows")]
#[used]
#[link_section = ".perrystr$a"]
static IMAGE_START: [u64; 1] = [0];
#[cfg(target_os = "windows")]
#[used]
#[link_section = ".perrystr$z"]
static IMAGE_END: [u64; 1] = [0];

#[inline]
fn bounds() -> (usize, usize) {
    #[cfg(target_vendor = "apple")]
    {
        unsafe extern "C" {
            #[link_name = "section$start$__TEXT$__perry_strings"]
            static START: u8;
            #[link_name = "section$end$__TEXT$__perry_strings"]
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
    #[cfg(not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")))]
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
    #[cfg(target_family = "wasm")]
    {
        (0, 0)
    }
}

/// Validate every byte range BEFORE reading it. An aligned forged pointer
/// inside the section may name padding or string data, so bounds alone do not
/// certify a header. Its marker, layout, length and complete payload must agree.
#[cold]
#[inline(never)]
pub(super) unsafe fn header(addr: usize) -> Option<std::ptr::NonNull<GcHeader>> {
    use crate::string::StringHeader;
    let (start, end) = bounds();
    let header_addr = addr.checked_sub(GC_HEADER_SIZE)?;
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
    if payload_end > end
        || string.byte_len != string.capacity
        || string.utf16_len > string.byte_len
        || gc.size as usize != payload_end - header_addr
        || *(payload_end as *const u8).sub(1) != 0
    {
        return None;
    }
    std::ptr::NonNull::new(header_addr as *mut GcHeader)
}
