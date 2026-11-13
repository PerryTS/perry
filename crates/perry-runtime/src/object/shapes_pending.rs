use super::*;

/// Is this word in the ordinary band's numeric comparison range? A writer
/// additionally requires [`is_site_fillable_word`], which excludes pending
/// cells. Dictionary, exotic, shadowed and non-layout words never match.
#[inline]
pub(crate) fn is_site_matchable_layout_word(v: u32) -> bool {
    (crate::codegen_abi::LAYOUT_TAG..crate::codegen_abi::LAYOUT_ORDINARY_END).contains(&v)
}

/// A site word must describe an already-bound ordinary receiver.
#[inline]
pub(crate) fn is_site_fillable_word(v: u32) -> bool {
    let ordinary_mask = !(crate::codegen_abi::LAYOUT_ORDINARY_END - crate::codegen_abi::LAYOUT_TAG - 1);
    v & (ordinary_mask | crate::codegen_abi::LAYOUT_ROW_PENDING) == crate::codegen_abi::LAYOUT_TAG
}

/// A PIC token obeys the same admission rule as its receiver word.
#[inline]
pub(crate) fn is_site_fillable_token(token: u64) -> bool {
    token == (PIC_ID_TOKEN_BIT | u64::from(token as u32))
        && is_site_fillable_word(token as u32)
}

/// [`is_site_matchable_layout_word`] for a per-site PIC TOKEN
/// (`PIC_ID_TOKEN_BIT | ShapeId`): the exact token form over a matchable id.
#[inline]
pub(crate) fn is_site_matchable_token(token: u64) -> bool {
    token == (PIC_ID_TOKEN_BIT | u64::from(token as u32))
        && is_site_matchable_layout_word(token as u32)
}

/// Collector reads never materialize an image cell. Pending cells contain
/// only immortal edges until their first program observation.
#[inline(always)]
pub(crate) unsafe fn object_shape_record_in_gc(
    obj: *const crate::object::ObjectHeader,
) -> Option<ShapeRecordRef> {
    shape_record_of_object(std::ptr::null(), object_shape_stamp(obj).0)
}
