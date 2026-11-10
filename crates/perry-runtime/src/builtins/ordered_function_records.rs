//! ELF SHF_LINK_ORDER follows body addresses; search the records themselves.
use super::*;

pub(super) fn predecessor(
    bounds: &FunctionRecordBounds,
    ip: usize,
) -> Option<(usize, &'static [u8])> {
    let start = bounds.start as usize;
    let count = (bounds.stop as usize).saturating_sub(start) / FUNCTION_RECORD_BYTES;
    let sentinel = (bounds.sentinel as usize)
        .checked_sub(start)
        .filter(|offset| offset % FUNCTION_RECORD_BYTES == 0)
        .map(|offset| offset / FUNCTION_RECORD_BYTES)
        .filter(|index| *index < count);
    // The descriptor names the sentinel, so it need not be first or last.
    let len = count - usize::from(sentinel.is_some());
    let at = |index: usize| {
        let physical = index + usize::from(sentinel.is_some_and(|s| index >= s));
        let record = start + physical * FUNCTION_RECORD_BYTES;
        // SAFETY: this slot is inside the immutable, linker-bounded section.
        let delta = unsafe { (record as *const i32).read() };
        if delta == 0 {
            return None;
        }
        // SAFETY: the codegen record names its body's complete static info.
        unsafe { (record.wrapping_add_signed(delta as isize) as *const JsFunctionInfo).as_ref() }
    };
    let (mut lo, mut hi) = (0, len);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        // Zero marker/alignment slots have no address and no search key.
        let next = (mid..hi).find_map(|index| at(index).map(|info| (index, info)));
        match next {
            Some((index, info)) if info.code as usize <= ip => lo = index + 1,
            _ => hi = mid,
        }
    }
    // Empty names cannot hide a preceding named body. Equal addresses retain
    // section order, just as max_by_key's last-on-equal scan does.
    (0..lo).rev().find_map(|index| {
        let info = at(index)?;
        let name = super::super::fn_metadata::info_name_bytes(info)?;
        (!name.is_empty()).then_some((info.code as usize, name))
    })
}
