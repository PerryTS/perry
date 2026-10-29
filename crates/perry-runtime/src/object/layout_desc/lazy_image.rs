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

/// A live image receiver can be encountered before any runtime birth of its
/// kind. Publish its row at that first read using the normal registrar. Held
/// words and identity probes do not call this path, avoiding publication while
/// the shape table is already being modified.
#[cold]
#[inline(never)]
pub(crate) fn publish_receiver(word: u32) {
    let Some(desc) = descriptor(word) else { return };
    let published = unsafe { register_one(desc, 0) };
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
