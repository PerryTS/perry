//! Rooted GC-array snapshots for native stream code that walks a list of JS
//! values while it calls code that can collect: user stage functions, `_read`
//! and `_write`, event listeners, `Readable.from` and plain allocation.
//!
//! A Rust `Vec<f64>` copy of such a list is invisible to the collector. A
//! moving collection leaves every entry naming its old address, and a
//! collection that does not move can free an entry whose only holder is the
//! copy. The snapshot here is a GC array held in a [`RuntimeHandle`], so the
//! collector both keeps its entries alive and rewrites them; callers read each
//! entry from it just before using it.

use super::*;
use crate::array::ArrayHeader;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};

/// The array a rooted array handle holds now. Valid until the next call that
/// can collect.
fn live_array(array: &RuntimeHandle<'_>) -> *const ArrayHeader {
    raw_ptr_from_value(array.get_nanbox_f64()) as *const ArrayHeader
}

/// The length of the rooted array `array`.
pub(super) fn rooted_array_len(array: &RuntimeHandle<'_>) -> u32 {
    crate::array::js_array_length(live_array(array))
}

/// Element `i` of the rooted array `array`, read from its current address.
pub(super) fn rooted_array_at(array: &RuntimeHandle<'_>, i: u32) -> f64 {
    crate::array::js_array_get_f64(live_array(array), i)
}

/// Overwrite element `i` of the rooted array `array`.
pub(super) fn rooted_array_set(array: &RuntimeHandle<'_>, i: u32, value: f64) {
    crate::array::js_array_set_f64(live_array(array) as *mut ArrayHeader, i, value);
}

/// Append `value` to the rooted array `array`. `js_array_push_f64` roots its
/// receiver and the value before it can grow.
pub(super) fn rooted_array_push(array: &RuntimeHandle<'_>, value: f64) {
    let grown = crate::array::js_array_push_f64(live_array(array) as *mut ArrayHeader, value);
    array.set_nanbox_f64(box_pointer(grown as *const u8));
}

/// A new closure for `func` capturing `captures`, rooted in `scope`. Each
/// capture is read from its handle after the allocation.
pub(super) fn rooted_closure<'s>(
    scope: &'s RuntimeHandleScope,
    func: *const crate::closure::JsFunctionInfo,
    captures: &[&RuntimeHandle<'_>],
) -> RuntimeHandle<'s> {
    let closure = crate::closure::js_closure_alloc(func, captures.len() as u32);
    for (i, capture) in captures.iter().enumerate() {
        crate::closure::js_closure_set_capture_f64(closure, i as u32, capture.get_nanbox_f64());
    }
    scope.root_nanbox_f64(box_pointer(closure as *const u8))
}

/// A new empty GC array rooted in `scope`.
pub(super) fn rooted_empty_array<'s>(scope: &'s RuntimeHandleScope) -> RuntimeHandle<'s> {
    scope.root_nanbox_f64(box_pointer(crate::array::js_array_alloc(0) as *const u8))
}

/// A copy of the first `len` elements (at most) of the array held in
/// `source`, rooted in `scope`. `source` stays rooted while the copy
/// allocates.
pub(super) fn rooted_array_copy<'s>(
    scope: &'s RuntimeHandleScope,
    source: &RuntimeHandle<'_>,
    len: u32,
) -> RuntimeHandle<'s> {
    let len = rooted_array_len(source).min(len);
    let copy = scope.root_nanbox_f64(box_pointer(crate::array::js_array_alloc(len) as *const u8));
    for i in 0..len {
        rooted_array_push(&copy, rooted_array_at(source, i));
    }
    copy
}

/// A copy of the native argument array `args` (null is empty), rooted in
/// `scope`. `args` is rooted before the copy allocates.
pub(super) fn rooted_args_copy<'s>(
    scope: &'s RuntimeHandleScope,
    args: *const ArrayHeader,
) -> RuntimeHandle<'s> {
    if args.is_null() {
        return rooted_empty_array(scope);
    }
    let source = scope.root_nanbox_f64(box_pointer(args as *const u8));
    rooted_array_copy(scope, &source, u32::MAX)
}

/// The chunk values of `value`, as `push_chunk_values` lists them, copied
/// into a GC array rooted in `scope`: a readable's buffered chunks, an
/// array's elements, or a single chunk.
pub(super) fn chunk_values_snapshot<'s>(
    scope: &'s RuntimeHandleScope,
    value: f64,
) -> RuntimeHandle<'s> {
    // Follow `push_chunk_values`' chain to the array or chunk it lists. Hidden
    // reads only; nothing here allocates until the copy below.
    let mut current = value;
    for _ in 0..=8 {
        match readable_hidden_chunks(current) {
            Some(chunks) => current = chunks,
            None => break,
        }
    }
    let source = scope.root_nanbox_f64(current);
    if is_array_like_value(current) {
        return rooted_array_copy(scope, &source, u32::MAX);
    }
    let out = rooted_empty_array(scope);
    if is_single_chunk_value(source.get_nanbox_f64()) {
        rooted_array_push(&out, source.get_nanbox_f64());
    }
    out
}
