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

#[inline]
pub(super) fn descriptor_count() -> u32 {
    count(layout_bounds()).saturating_add(count(bounds()))
}

/// Arithmetic only: eager image records precede first-birth image records.
#[inline]
pub(super) fn descriptor_at(index: u32) -> *const PerryLayoutDesc {
    let eager = layout_bounds();
    let n = count(eager);
    let (base, offset) = if index < n {
        (eager.0, index)
    } else {
        (bounds().0, index - n)
    };
    (base + offset as usize * std::mem::size_of::<PerryLayoutDesc>()) as *const PerryLayoutDesc
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
#[inline]
pub(super) unsafe fn registered_for_birth(desc: *const PerryLayoutDesc) -> Option<u32> {
    let current = registered_layout_id(load_layout_id(desc));
    let Some(index) = literal_index(desc) else {
        return current;
    };
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
    if !optional_family::is_family(d) || d.family == SENTINEL_FAMILY {
        return None;
    }
    let requested = layout_id_of_index(index)?;
    let id = register_one(desc, requested);
    (id != 0 && super::super::shapes::shape_descriptor_by_id(id).is_some()).then_some(id)
}
