//! Literal-family descriptors are image data, published only at first birth.
//! The two image spans form one arithmetic descriptor index, with the existing
//! ShapeId band and no address registry or per-agent validity state.
use super::*;

#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_litlay")]
#[cfg_attr(target_os = "windows", link_section = ".perrylit$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_literal_layouts"
)]
static mut SENTINEL: PerryLayoutDesc = PerryLayoutDesc {
    family: SENTINEL_FAMILY,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 0,
    layout_id: 0,
    rep: 0,
    proto: LAYOUT_PROTO_OBJECT,
    keys: std::ptr::null(),
    key_count: 0,
    index_mask: 0,
    index: std::ptr::null(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};

#[cfg(target_os = "windows")]
#[used(linker)]
#[link_section = ".perrylit$a"]
static START: [u64; 1] = [0];
#[cfg(target_os = "windows")]
#[used(linker)]
#[link_section = ".perrylit$z"]
static END: [u64; 1] = [0];

#[inline]
fn bounds() -> (usize, usize) {
    std::hint::black_box(std::ptr::addr_of!(SENTINEL) as usize);
    #[cfg(target_vendor = "apple")]
    {
        unsafe extern "C" {
            #[link_name = "\u{1}section$start$__DATA$__perry_litlay"]
            static START: u8;
            #[link_name = "\u{1}section$end$__DATA$__perry_litlay"]
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
            std::ptr::addr_of!(START) as usize + 8,
            std::ptr::addr_of!(END) as usize,
        )
    }
    #[cfg(not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")))]
    {
        unsafe extern "C" {
            static __start_perry_literal_layouts: u8;
            static __stop_perry_literal_layouts: u8;
        }
        (
            std::ptr::addr_of!(__start_perry_literal_layouts) as usize,
            std::ptr::addr_of!(__stop_perry_literal_layouts) as usize,
        )
    }
    #[cfg(target_family = "wasm")]
    {
        (0, 0)
    }
}

#[inline]
fn count((start, end): (usize, usize)) -> u32 {
    (end.saturating_sub(start) / std::mem::size_of::<PerryLayoutDesc>()) as u32
}

/// One checked byte-offset decoder for both immutable image spans. The
/// boolean is the selected span's classless-family authority, not object state.
/// Comparing byte offsets avoids dividing linker span lengths on every read.
#[inline]
pub(super) fn descriptor_at(index: u32) -> Option<(*const PerryLayoutDesc, bool)> {
    if index >= LAYOUT_INDEX_LIMIT {
        return None;
    }
    let eager = layout_bounds();
    let bytes = eager.1.saturating_sub(eager.0);
    // LAYOUT_INDEX_LIMIT bounds this multiplication even on 32-bit targets.
    let offset = index as usize * std::mem::size_of::<PerryLayoutDesc>();
    if offset < bytes {
        return Some(((eager.0 + offset) as *const PerryLayoutDesc, false));
    }
    let literal = bounds();
    let offset = offset - bytes;
    if offset >= literal.1.saturating_sub(literal.0) {
        return None;
    }
    Some(((literal.0 + offset) as *const PerryLayoutDesc, true))
}

/// Generic words need family classification as well as a checked decode.
/// The literal section owns classless identity; eager families are nominal.
#[inline]
pub(super) fn family_at(index: u32) -> Option<*const PerryLayoutDesc> {
    let (desc, literal) = descriptor_at(index)?;
    unsafe { (literal || (*desc).class_id != 0).then_some(desc) }
}

/// Section ownership is the immutable classless-family authority.
#[inline]
pub(super) fn contains(desc: *const PerryLayoutDesc) -> bool {
    let (start, end) = bounds();
    let address = desc as usize;
    address >= start
        && address < end
        && (address - start) % std::mem::size_of::<PerryLayoutDesc>() == 0
}

#[inline]
fn literal_index(desc: *const PerryLayoutDesc) -> Option<u32> {
    let (start, end) = bounds();
    let address = desc as usize;
    let size = std::mem::size_of::<PerryLayoutDesc>();
    if address < start || address >= end || (address - start) % size != 0 {
        return None;
    }
    let index = count(layout_bounds()) + ((address - start) / size) as u32;
    (index < LAYOUT_INDEX_LIMIT).then_some(index)
}

/// Check the current agent's authoritative record, even when another agent
/// already published the descriptor's existing image word.
#[inline(never)]
pub(super) unsafe fn registered_for_birth(desc: *const PerryLayoutDesc) -> Option<u32> {
    let index = literal_index(desc)?;
    let current = registered_layout_id(load_layout_id(desc));
    if let Some(id) = current {
        if super::super::shapes::shape_descriptor_by_id(id).is_some() {
            return Some(id);
        }
    }
    first_birth(desc, index)
}

/// This path must be absent from startup when none of these literals is born.
/// Registration has the existing suppressed-GC install operation and creates
/// compatibility keys only now; the descriptor and its index stay in the image.
#[inline(never)]
unsafe fn first_birth(desc: *const PerryLayoutDesc, index: u32) -> Option<u32> {
    let d = &*desc;
    if !optional_family::valid_literal(d) || d.family == SENTINEL_FAMILY {
        return None;
    }
    let requested = layout_id_of_index(index)?;
    let id = register_one(desc, requested);
    (id != 0 && super::super::shapes::shape_descriptor_by_id(id).is_some()).then_some(id)
}

/// Birth of a literal-section family. The section owns the schema, while
/// the current agent owns its registered ShapeRecord. Ordinary registered
/// descriptors use perry_alloc_layout and never enter publication here.
///
/// # Safety
/// desc points to this program's immutable literal descriptor image.
#[no_mangle]
pub unsafe extern "C" fn perry_alloc_literal_layout(
    desc: *const PerryLayoutDesc,
) -> *mut ObjectHeader {
    #[cfg(test)]
    crate::test_support::bootstrap_test_thread();
    let Some(id) = registered_for_birth(desc) else {
        eprintln!(
            "Perry internal error: literal birth requires a valid literal-section descriptor"
        );
        std::process::abort();
    };
    alloc_layout_object(&*desc, id)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_ALLOC_LITERAL_LAYOUT: unsafe extern "C" fn(
    *const PerryLayoutDesc,
) -> *mut ObjectHeader = perry_alloc_literal_layout;
