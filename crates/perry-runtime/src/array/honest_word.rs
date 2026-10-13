//! Array-entry words under honest pointers: a `POINTER_TAG` word names a
//! GC-headed object, so its header is the proof; a raw word keeps the
//! allocator's admission.

/// The user address of an arena GC object named by an array-entry word, or
/// `None`. A `POINTER_TAG` word is honest: its header's arena flag is the
/// whole proof. A raw word must be a plausible, aligned address on an arena
/// page this thread owns before its header may be read. Other tags are none.
#[inline(always)]
pub(crate) fn arena_object_of_word(bits: u64) -> Option<usize> {
    if let Some(header) = crate::value::addr_class::pointer_value_header(bits) {
        return (header.gc_flags & crate::gc::GC_FLAG_ARENA != 0)
            .then_some((bits & crate::value::POINTER_MASK) as usize);
    }
    let raw = bits as usize;
    (bits >> 48 == 0
        && raw >= crate::gc::GC_HEADER_SIZE
        && raw % std::mem::align_of::<crate::gc::GcHeader>() == 0
        && crate::value::addr_class::is_plausible_heap_addr(raw)
        && !matches!(
            crate::arena::classify_heap_generation(raw),
            crate::arena::HeapGeneration::Unknown
        ))
    .then_some(raw)
}

/// The array-entry argument for a JS value: a `POINTER_TAG` word as-is, so
/// [`super::clean_arr_ptr`] answers from its honest header; any other word decoded
/// as before (raw words keep their admission).
#[inline(always)]
pub(crate) fn array_arg_of_value(value: f64) -> *const super::ArrayHeader {
    let bits = value.to_bits();
    if bits & crate::value::TAG_MASK == crate::value::POINTER_TAG {
        bits as *const super::ArrayHeader
    } else {
        crate::value::js_nanbox_get_pointer(value) as *const super::ArrayHeader
    }
}
