//! Literal-family descriptors are image data, published only at first birth.
//! Both image spans share one checked descriptor index. Header words belong
//! to DPA's whole-image row plan; the agent record owns the descriptor index.
use super::*;

#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_litlay")]
#[cfg_attr(target_os = "windows", link_section = ".perrylit$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_literal_layouts"
)]
static mut SENTINEL: PerryLayoutDesc = PerryLayoutDesc {
    image_keys: std::ptr::null_mut(),
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
    shape_id: 0,
    _layout_pad: 0,
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
    #[cfg(not(any(target_vendor = "apple", target_os = "windows")))]
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
    let literal_bytes = literal.1.saturating_sub(literal.0);
    if offset >= literal_bytes {
        // Runtime inventory follows both program spans. Program births only
        // subtract linker bounds; they never walk or count the inventory.
        #[cfg(target_family = "wasm")]
        return super::enumeration::enumerated_layout_at(
            inventory::iter::<crate::wasm_image::Layout>
                .into_iter()
                .map(|item| item.0),
            ((offset - literal_bytes) / std::mem::size_of::<PerryLayoutDesc>()) as u32,
        )
        .map(|desc| (desc, false));
        #[cfg(not(target_family = "wasm"))]
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
pub(super) fn literal_index(desc: *const PerryLayoutDesc) -> Option<u32> {
    let (start, end) = bounds();
    let address = desc as usize;
    let size = std::mem::size_of::<PerryLayoutDesc>();
    if address < start || address >= end || (address - start) % size != 0 {
        return None;
    }
    let eager = count(layout_bounds());
    let index = eager + ((address - start) / size) as u32;
    (index < LAYOUT_INDEX_LIMIT).then_some(index)
}

/// Runtime descriptors retain their existing inventory; they follow both
/// linker-owned program spans in the same checked index space.
#[cfg(target_family = "wasm")]
pub(super) fn runtime_index(desc: *const PerryLayoutDesc) -> Option<u32> {
    let position = super::enumeration::enumerated_layout_index(
        inventory::iter::<crate::wasm_image::Layout>
            .into_iter()
            .map(|item| item.0),
        desc,
    )?;
    let index = count(layout_bounds())
        .checked_add(count(bounds()))?
        .checked_add(position)?;
    (index < LAYOUT_INDEX_LIMIT).then_some(index)
}

#[cfg(all(feature = "wasm-image-selftest", target_family = "wasm"))]
pub(super) fn check_image_indices() {
    let eager = records::<PerryLayoutDesc>(layout_bounds()).map(|desc| (desc, false));
    let literal = records::<PerryLayoutDesc>(bounds()).map(|desc| (desc, true));
    let runtime = inventory::iter::<crate::wasm_image::Layout>
        .into_iter()
        .map(|item| (item.0, false));
    let mut checked = 0;
    for (desc, expected_literal) in eager.chain(literal).chain(runtime) {
        let index = descriptor_index(desc).expect("image descriptor has an index");
        let (decoded, is_literal) = descriptor_at(index).expect("image index decodes");
        assert!(std::ptr::eq(desc, decoded), "descriptor index round trip");
        assert_eq!(is_literal, expected_literal, "section owns the family kind");
        checked += 1;
    }
    eprintln!("wasm layout bounds: {checked} descriptor index round trips passed");
}

/// Check the current agent's authoritative record, even when another agent
/// already published the descriptor's existing image word.
#[inline(never)]
pub(super) unsafe fn registered_for_birth(desc: *const PerryLayoutDesc) -> Option<u32> {
    let index = literal_index(desc)?;
    let current = registered_layout_id(load_layout_id(desc));
    if let Some(id) = current {
        if super::super::shapes::shape_descriptor_for_word(LayoutWord(id)).is_some() {
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
    // The immutable word comes from the whole-image DPA ordinal plan. The
    // existing identity allocator and agent Region publish its shape facts.
    let requested = super::super::static_shapes::resolve_layout_fields(
        std::ptr::addr_of!((*desc).shape_id).cast_mut(),
        LayoutWord(d.layout_id),
        super::super::shapes::PROTO_ID_DEFAULT,
    )
    .identity
    .0;
    debug_assert_eq!(literal_index(desc), Some(index));
    let id = register_one(desc, requested);
    (id != 0 && super::super::shapes::shape_descriptor_for_word(LayoutWord(id)).is_some())
        .then_some(id)
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
    alloc_layout_object(&*desc, LayoutWord(id))
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_ALLOC_LITERAL_LAYOUT: unsafe extern "C" fn(
    *const PerryLayoutDesc,
) -> *mut ObjectHeader = perry_alloc_literal_layout;
