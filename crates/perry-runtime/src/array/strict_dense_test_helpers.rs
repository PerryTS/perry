//! Test-only strict-dense store helpers, split out of `indexing.rs` to keep
//! it under the 2000-line cap. `#![cfg(test)]` at module level, so the
//! per-item `#[cfg(test)]` attributes the originals carried are dropped.

#![cfg(test)]

use super::indexing::{try_strict_dense_number_store, try_strict_dense_pointer_overwrite};
use super::*;

thread_local! {
    static STRICT_DENSE_POINTER_OVERWRITE_HITS: std::cell::Cell<u64> = const {
        std::cell::Cell::new(0)
    };
    static ELEMENT_ACCESSOR_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

pub(crate) fn note_strict_dense_pointer_overwrite_hit() {
    STRICT_DENSE_POINTER_OVERWRITE_HITS.with(|hits| hits.set(hits.get().wrapping_add(1)));
}

pub(crate) fn test_strict_dense_pointer_overwrite_hits() -> u64 {
    STRICT_DENSE_POINTER_OVERWRITE_HITS.with(std::cell::Cell::get)
}

// Entry counter for `js_array_get_f64`, the JS-facing element accessor. Tests
// use it to prove internal walks avoid the full per-element resolver gauntlet.
pub(crate) fn note_element_accessor_call() {
    ELEMENT_ACCESSOR_CALLS.with(|calls| calls.set(calls.get().wrapping_add(1)));
}

pub(crate) fn test_element_accessor_calls() -> u64 {
    ELEMENT_ACCESSOR_CALLS.with(std::cell::Cell::get)
}

pub(crate) fn test_strict_dense_pointer_overwrite(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> bool {
    unsafe { try_strict_dense_pointer_overwrite(arr, index, value) }.is_some()
}

pub(crate) fn test_strict_dense_number_store(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> bool {
    unsafe { try_strict_dense_number_store(arr, index, value) }.is_some()
}
