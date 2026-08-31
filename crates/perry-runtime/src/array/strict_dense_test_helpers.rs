//! Test-only strict-dense store helpers, split out of `indexing.rs` to keep
//! it under the 2000-line cap. `#![cfg(test)]` at module level, so the
//! per-item `#[cfg(test)]` attributes the originals carried are dropped.

#![cfg(test)]

use super::indexing::{try_strict_dense_number_store, try_strict_dense_pointer_overwrite};
use super::*;

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
