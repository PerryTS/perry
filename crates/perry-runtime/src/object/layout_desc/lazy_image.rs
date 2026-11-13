//! Static receiver rows address their descriptor facts directly. This is the
//! existing descriptor publication mechanism with an image-selected row;
//! there is no descriptor registry, row-to-pointer table or section scan.
use super::*;
#[path = "lazy_image/family_bind.rs"]
mod family_bind;
#[cfg(any(debug_assertions, test))]
pub(crate) use family_bind::holds as holds_image_family;
pub(crate) use family_bind::seed as seed_image_family;

/// This agent's program table, when its image carries a lazy span.
#[inline(always)]
fn span() -> Option<&'static crate::codegen_abi::PerryProgramTableV1> {
    // SAFETY: a bound image names its program's immutable table.
    let table = unsafe { &*super::super::class_image::agent_layout_image().program };
    (table.lazy_count != 0).then_some(table)
}

/// The descriptor for a lazy static row of this agent's executable image.
#[inline]
fn descriptor(word: u32) -> Option<*const PerryLayoutDesc> {
    let span = span()?;
    let row = LayoutWord(word).row().0;
    let offset = row.checked_sub(span.lazy_first)?;
    if offset >= span.lazy_count {
        return None;
    }
    // The validated program table owns this contiguous array for its lifetime.
    let desc = unsafe {
        span.lazy_layouts
            .cast::<PerryLayoutDesc>()
            .add(offset as usize)
    };
    (unsafe { (*desc).layout_id } == LayoutWord(word).unshadowed().0).then_some(desc)
}

/// Whether `desc` is the descriptor value selected by its static row.
#[inline]
pub(super) unsafe fn contains(desc: *const PerryLayoutDesc) -> bool {
    descriptor((*desc).layout_id).is_some_and(|owned| std::ptr::eq(owned, desc))
}

/// The byte range of `span`'s descriptors inside the layout section
/// `[start, end)`, or an empty range at `end`. The startup registrar walks
/// around it.
#[inline]
pub(super) fn section_span(
    span: &crate::codegen_abi::PerryProgramTableV1,
    start: usize,
    end: usize,
) -> (usize, usize) {
    let first = span.lazy_layouts as usize;
    let last = first + span.lazy_count as usize * std::mem::size_of::<PerryLayoutDesc>();
    if first < start || last > end {
        return (end, end);
    }
    (first, last)
}

/// The exotic kind of a lazy receiver row (function or native namespace),
/// which declared program layouts never carry. None for any other row.
#[cold]
pub(super) unsafe fn receiver_kind(
    desc: *const PerryLayoutDesc,
) -> Option<super::super::shapes::ShapeObjectKind> {
    use super::super::shapes::ShapeObjectKind::*;
    if !contains(desc) {
        return None;
    }
    [
        Function,
        FunctionBoundCall,
        FunctionBoundApply,
        FunctionBound,
        NativeNamespace,
    ]
    .into_iter()
    .find(|kind| kind.code() == u64::from((*desc).kind))
}

/// A live image receiver can be encountered before any runtime birth of its
/// kind. Publish its row at that first read using the normal registrar. Held
/// words and identity probes do not call this path, avoiding publication while
/// the shape table is already being modified.
#[cold]
#[optimize(size)]
#[inline(never)]
pub(crate) fn publish_receiver(word: u32) {
    let Some(desc) = descriptor(word) else { return };

    let published = unsafe { super::registration::register_lazy_row(desc) };
    assert_eq!(
        published,
        LayoutWord(word).unshadowed().0,
        "realm receiver has invalid descriptor facts at static row {}",
        LayoutWord(word).row().0
    );
    assert!(
        super::super::shapes::shape_record_for_word(LayoutWord(published)).is_some(),
        "realm receiver descriptor did not publish its static row"
    );
}

/// The pending word of a proven ordinary prototype or closure header.
#[inline(always)]
pub(crate) unsafe fn pending_intrinsic_word(addr: usize) -> Option<u32> {
    if addr == 0 {
        return None;
    }
    let word = *(addr as *const u32).add(1);
    (word & crate::codegen_abi::LAYOUT_ROW_PENDING != 0).then_some(word)
}

/// The pending logical row of a proven closure, including a dictionary bag.
///
/// # Safety
/// A nonzero address names a live closure whose props pointer is valid.
#[inline]
pub(crate) unsafe fn pending_closure_cell(addr: usize) -> Option<(*mut u8, u32)> {
    if addr == 0 {
        return None;
    }
    if let Some(word) = pending_intrinsic_word(addr) {
        return Some((addr as *mut u8, word));
    }
    let owner = (*(addr as *const crate::closure::ClosureHeader)).props as usize;
    pending_intrinsic_word(owner).map(|word| (owner as *mut u8, word))
}

/// The logical layout owner of an image intrinsic. Native dictionary
/// functions carry their own-key row in props; arrays use their named holder.
#[inline]
pub(crate) unsafe fn pending_cell(addr: usize) -> Option<(*mut u8, u32)> {
    if addr == 0 || !crate::value::addr_class::realm::is_image_object(addr) {
        return None;
    }
    let header = &*((addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
    let owner = match header.obj_type {
        crate::gc::GC_TYPE_OBJECT => addr,
        crate::gc::GC_TYPE_CLOSURE => return pending_closure_cell(addr),
        crate::gc_array_type_pat!() if header.gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED == 0 => {
            crate::array::array_property_bag_resolved(addr as *const crate::array::ArrayHeader)
                as usize
        }
        _ => return None,
    };
    pending_intrinsic_word(owner).map(|word| (owner as *mut u8, word))
}

/// First observation of an image receiver publishes its emitted row.
///
/// # Safety
/// `obj` is a live image receiver carrying the supplied pending `word`.
#[no_mangle]
pub unsafe extern "C" fn js_realm_touch_pending(obj: *mut u8, word: u32) {
    assert!(
        crate::gc::image_cells::agent_uses_image(),
        "pending cell on an agent without an image"
    );
    assert!(!obj.is_null());
    assert!(crate::value::addr_class::realm::is_image_object(
        obj as usize
    ));
    #[cfg(any(test, debug_assertions))]
    let _no_gc = super::NoGcAlloc::enter();
    let obj = obj.cast::<crate::object::ObjectHeader>();
    debug_assert_eq!((*obj).parent_class_id, word);
    family_bind::bind(obj, word);
}

#[cfg(test)]
#[path = "lazy_image/tests.rs"]
mod tests;
