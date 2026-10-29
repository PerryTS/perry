//! Static receiver rows address their descriptor facts directly. This is the
//! existing descriptor publication mechanism with an image-selected row;
//! there is no descriptor registry, row-to-pointer table or section scan.
use super::*;

/// The descriptor for a lazy static row of this agent's executable image.
#[inline]
fn descriptor(word: u32) -> Option<*const PerryLayoutDesc> {
    let image = super::super::class_image::agent_layout_image();
    let row = LayoutWord(word).row().0;
    let offset = row.checked_sub(image.lazy_first)?;
    if offset >= image.lazy_count {
        return None;
    }
    // The validated image header owns this contiguous array for its lifetime.
    let desc = unsafe {
        image
            .lazy_layouts
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

/// The byte range of the lazy span inside the layout section `[start, end)`,
/// or an empty range at `end`. The startup registrar walks around it.
#[inline]
pub(super) fn section_span(start: usize, end: usize) -> (usize, usize) {
    let image = super::super::class_image::agent_layout_image();
    if image.lazy_count == 0 {
        return (end, end);
    }
    let first = image.lazy_layouts as usize;
    let last = first + image.lazy_count as usize * std::mem::size_of::<PerryLayoutDesc>();
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

/// An image family carries its serial and its image prototype. The row that
/// names it binds the prototype through the agent's identity word at first
/// touch; a worker's already-bound prototype is never replaced.
#[cold]
unsafe fn bind_family_image_prototype(desc: *const PerryLayoutDesc) {
    let proto = (*desc).proto;
    if (proto as usize) < LAYOUT_PROTO_TAG_END {
        return;
    }
    let family = proto.cast::<PerryProtoFamily>();
    let prototype = (*family).image_prototype;
    if prototype.is_null() {
        return;
    }
    let serial = (*family).serial;
    assert!(serial != 0, "image prototype lacks its serial");
    if super::super::shapes::identity_prototype_word(serial) == 0 {
        super::super::shapes::write_identity_word(
            serial,
            crate::value::JSValue::pointer(prototype).bits(),
        );
    }
}

/// A live image receiver can be encountered before any runtime birth of its
/// kind. Publish its row at that first read using the normal registrar. Held
/// words and identity probes do not call this path, avoiding publication while
/// the shape table is already being modified.
// The production entry (WRT3b items 2-6) chooses where a first touch calls
// this without a test on the receiver read path; until then only tests do.
#[cfg_attr(not(test), allow(dead_code))]
#[cold]
#[inline(never)]
pub(crate) fn publish_receiver(word: u32) {
    let Some(desc) = descriptor(word) else { return };
    unsafe { bind_family_image_prototype(desc) };
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

#[cfg(test)]
#[path = "lazy_image/tests.rs"]
mod tests;
