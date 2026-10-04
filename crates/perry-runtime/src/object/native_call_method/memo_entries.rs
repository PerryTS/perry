//! The computed-key method call's entries for a site that owns a chain memo
//! slot (`method_site::chain_memo`): `obj[k](...)` where codegen knows the
//! key is a string (`str_key`) or holds any value (`value`). Each forwards to
//! the dispatch its memo-less sibling runs, with the site's memo, keyed by
//! the key's bytes.

use crate::object::method_site::chain_memo::{memo_ref_slot, ChainMemoSlot, MemoRef};

/// [`super::js_native_call_method_str_key`] for a site with a chain memo slot.
///
/// # Safety
/// As the sibling; `memo` is null or the site's live memo slot.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_native_call_method_str_key_memo(
    object: f64,
    name_handle: i64,
    args_ptr: *const f64,
    args_len: usize,
    memo: *mut ChainMemoSlot,
) -> f64 {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(name_ref) =
        crate::string::perry_string_ref_from_dispatch_id(name_handle, &mut scratch)
    else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    memo_call(
        object,
        name_ref.ptr as *const i8,
        name_ref.len,
        args_ptr,
        args_len,
        memo_ref_slot(memo, true),
        false,
    )
}

/// [`super::js_native_call_method_value`] for a site with a chain memo slot.
///
/// # Safety
/// As the sibling; `memo` is null or the site's live memo slot.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_native_call_method_value_memo(
    object: f64,
    key: f64,
    args_ptr: *const f64,
    args_len: usize,
    memo: *mut ChainMemoSlot,
) -> f64 {
    super::native_call_method_value_memo(object, key, args_ptr, args_len, memo_ref_slot(memo, true))
}

/// A by-name call at a site with a chain memo: the dispatch, with the memo
/// (`checked`: the caller has just asked the memo, so a miss there is not
/// asked again).
///
/// # Safety
/// As [`super::js_native_call_method`]; `memo` is 0 or names a live memo slot.
#[inline]
pub(super) unsafe fn memo_call(
    object: f64,
    name_ptr: *const i8,
    name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
    memo: MemoRef,
    checked: bool,
) -> f64 {
    let _ = checked;
    super::native_call_method_tower(object, name_ptr, name_len, args_ptr, args_len, memo)
}
