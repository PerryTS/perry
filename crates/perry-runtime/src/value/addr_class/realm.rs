//! The realm image (WRT, DESIGN §2.1/§2.3): GC-headed cells the compiler
//! places in one writable linker section, `perry_realm`. Image cells are
//! immortal and never move. A cell is an image cell exactly when its header
//! lies in the section; there is no header bit (`GcHeader._reserved` has none
//! free). Bounds come from the linker, under the same three schemes as
//! `perry_strings` (`image.rs`); nothing is registered or cached.
//!
//! The section is writable (programs patch builtins; a written page becomes
//! one private copy), so every runtime-side member of it is writable too:
//! COFF groups `$`-sorted inputs by their characteristics before sorting, and
//! a read-only sentinel or bound would delimit a separate section
//! (`object/layout_desc.rs` documents the same trap for `perry_layouts`).
use super::*;

// Always create the section, including in Rust tests and runtime-only links.
// The sentinel is one zero word: a walker reads it as a size-0 header and
// steps over it, exactly like the linker's zero padding between cells.
// `used(linker)` keeps it through `-dead_strip` / `--gc-sections` /
// `/OPT:REF`, and `bounds()` references it, so whatever needs the bounds
// brings the section (ld64 cannot synthesize `section$start` for a section
// it dropped).
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_realm")]
#[cfg_attr(target_os = "windows", link_section = ".perryrealm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_realm"
)]
static mut REALM_SECTION_SENTINEL: [u64; 1] = [0];

#[cfg(target_os = "windows")]
#[used(linker)]
#[link_section = ".perryrealm$a"]
static mut REALM_START: [u64; 1] = [0];
#[cfg(target_os = "windows")]
#[used(linker)]
#[link_section = ".perryrealm$z"]
static mut REALM_END: [u64; 1] = [0];

/// `[start, end)` of the realm section: every image cell's header lies in it.
#[inline]
pub(crate) fn bounds() -> (usize, usize) {
    // A real reference: whatever links this function links the sentinel too.
    std::hint::black_box(std::ptr::addr_of!(REALM_SECTION_SENTINEL));
    #[cfg(target_vendor = "apple")]
    {
        // ld64 synthesizes `section$start$SEG$SECT` only under that exact
        // name; the leading \x01 keeps LLVM from adding Mach-O's `_`.
        unsafe extern "C" {
            #[link_name = "\u{1}section$start$__DATA$__perry_realm"]
            static START: u8;
            #[link_name = "\u{1}section$end$__DATA$__perry_realm"]
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
            std::ptr::addr_of!(REALM_START) as usize + 8,
            std::ptr::addr_of!(REALM_END) as usize,
        )
    }
    // ELF and wasm-ld both synthesize these for the C-identifier section name.
    #[cfg(not(any(target_vendor = "apple", target_os = "windows")))]
    {
        unsafe extern "C" {
            static __start_perry_realm: u8;
            static __stop_perry_realm: u8;
        }
        (
            std::ptr::addr_of!(__start_perry_realm) as usize,
            std::ptr::addr_of!(__stop_perry_realm) as usize,
        )
    }
}

/// Is `addr` (a user address, `header + GC_HEADER_SIZE`) inside the realm
/// image? A range test on the header address: no load, no header bit.
#[inline]
pub(crate) fn is_image_object(addr: usize) -> bool {
    let (start, end) = bounds();
    let header = addr.wrapping_sub(GC_HEADER_SIZE);
    header >= start && header < end
}

/// Flags every image cell carries from birth (codegen's `IMAGE_GC_FLAGS`
/// minus the string-only INTERNED bit): mark and pin are data, so no
/// collector ever treats the cell as white or movable.
pub(crate) const IMAGE_CELL_FLAGS: u8 = crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_PINNED;

/// A well-formed image cell header at `header_addr`: a known GC type, the
/// image flags and no allocator flag, a size that is a multiple of eight and
/// ends inside the section.
#[inline]
unsafe fn cell_header_is_well_formed(header_addr: usize, end: usize) -> bool {
    let gc = &*(header_addr as *const GcHeader);
    let size = gc.size as usize;
    crate::gc::gc_type_is_known(gc.obj_type)
        && gc.gc_flags & (IMAGE_CELL_FLAGS | crate::gc::GC_FLAG_ARENA | crate::gc::GC_FLAG_FORWARDED)
            == IMAGE_CELL_FLAGS
        && size >= GC_HEADER_SIZE
        && size % 8 == 0
        && header_addr
            .checked_add(size)
            .is_some_and(|cell_end| cell_end <= end)
}

/// The header of the image cell whose user address is `addr`, or `None`.
///
/// Validates before reading anything beyond the header: the address must be
/// in the section and 8-aligned, and the header must be well formed. Like
/// the image-string check (`image::header`), this is a structural proof: an
/// aligned word inside a cell whose bytes happen to form a well-formed header
/// is not excluded. Exact cell starts are what [`for_each_cell`] walks.
#[cold]
#[inline(never)]
pub(crate) unsafe fn header(addr: usize) -> Option<std::ptr::NonNull<GcHeader>> {
    let (start, end) = bounds();
    let header_addr = addr.checked_sub(GC_HEADER_SIZE)?;
    if header_addr < start || header_addr >= end || addr % 8 != 0 {
        return None;
    }
    if !cell_header_is_well_formed(header_addr, end) {
        return None;
    }
    std::ptr::NonNull::new(header_addr as *mut GcHeader)
}

/// Visit every image cell in address order. The section is a sequence of
/// cells, each `GcHeader.size` bytes, with zero words between them (the
/// sentinel, the linker's alignment padding, Windows' `$a` group); a zero
/// header word is stepped over. Cells are laid out by codegen (and by the
/// runtime's test fixture), so a malformed header is a producer bug: the walk
/// stops there rather than guess.
#[inline(never)]
pub(crate) fn for_each_cell(visit: &mut dyn FnMut(*mut GcHeader)) {
    let (start, end) = bounds();
    let mut at = (start + 7) & !7;
    while at + GC_HEADER_SIZE <= end {
        // SAFETY: `at` is 8-aligned and inside the mapped section.
        let word = unsafe { std::ptr::read(at as *const u64) };
        if word == 0 {
            at += 8;
            continue;
        }
        if !unsafe { cell_header_is_well_formed(at, end) } {
            debug_assert!(false, "malformed realm image cell at {at:#x}: {word:#x}");
            return;
        }
        let size = unsafe { (*(at as *const GcHeader)).size as usize };
        visit(at as *mut GcHeader);
        at += size;
    }
}

/// The body of the realm fixture's function cell (WRT step 1). Codegen's
/// test-only emission path (`PERRY_REALM_TEST_FIXTURE`) points an image
/// closure's `info` at [`perry_realm_fixture_info`], as step 3 points each
/// builtin's cell at its `fn_info!` static; the runtime tests use it too.
/// Nothing references it otherwise, so ordinary links drop it.
extern "C" fn realm_fixture_body(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    42.0
}

#[allow(non_upper_case_globals)]
#[no_mangle]
pub static perry_realm_fixture_info: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(
        realm_fixture_body as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
    );

/// Publish the realm fixture object as `globalThis.__perryRealmFixture` (WRT
/// step 1, `PERRY_REALM_TEST_FIXTURE`), so a program can reach it before
/// step 3 makes the global object itself an image cell. Only the fixture's
/// `main` references it.
#[no_mangle]
pub extern "C" fn js_realm_test_fixture_publish(value: f64) {
    let global = crate::object::js_get_global_this().to_bits() & crate::value::POINTER_MASK;
    let key = b"__perryRealmFixture";
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_set_field_by_name(
        global as *mut crate::object::ObjectHeader,
        key,
        value,
    );
}
