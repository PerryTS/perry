//! Reuse the last long trim of an immutable source without another payload copy.
//!
//! StringHeader's inline payload is also the codegen/FFI ABI, so a cold trim
//! still copies its retained range. This single-entry, per-thread cache makes
//! repeated trims allocation-free without changing that representation. Both
//! pointers are strong, rewritable GC roots, never pinned. Replacement releases
//! the previous pair; the capacity budget bounds retained storage even for a
//! short string backed by a large append buffer. Small results are cheap to copy
//! and must not keep a large source alive just for a few bytes.

use super::*;
use std::cell::UnsafeCell;

pub(super) const MAX_RETAINED_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const MIN_CACHED_BYTES: u32 = 256;

struct TrimCache {
    source: *mut StringHeader,
    result: *mut StringHeader,
    mode: u8,
}

thread_local! {
    static TRIM_CACHE: UnsafeCell<TrimCache> = const { UnsafeCell::new(TrimCache {
        source: ptr::null_mut(),
        result: ptr::null_mut(),
        mode: 0,
    }) };
}

pub(super) fn lookup(source: *const StringHeader, mode: u8) -> *mut StringHeader {
    TRIM_CACHE.with(|cell| unsafe {
        let cache = &*cell.get();
        if ptr::eq(cache.source, source) && cache.mode == mode {
            cache.result
        } else {
            ptr::null_mut()
        }
    })
}

fn store(source: *mut StringHeader, result: *mut StringHeader, mode: u8) {
    TRIM_CACHE.with(|cell| unsafe {
        let cache = &mut *cell.get();
        // GC_STORE_AUDIT(ROOT): both slots are visited by scan_trim_cache_roots_mut.
        crate::gc::runtime_store_root_raw_mut_ptr_slot(&raw mut cache.source, source);
        crate::gc::runtime_store_root_raw_mut_ptr_slot(&raw mut cache.result, result);
        cache.mode = mode;
    });
}

pub(super) fn remember(source: *const StringHeader, result: *mut StringHeader, mode: u8) {
    unsafe {
        if (*result).byte_len < MIN_CACHED_BYTES {
            return;
        }
        let retained = u64::from((*source).capacity)
            + u64::from((*result).capacity)
            + 2 * std::mem::size_of::<StringHeader>() as u64;
        // FFI callers and guard-page tests can supply non-GC headers. A root
        // cannot extend their external lifetime, so never retain those pointers.
        if retained > MAX_RETAINED_BYTES
            || matches!(
                crate::arena::classify_heap_space(source as usize),
                crate::arena::HeapSpace::Unknown
            )
        {
            store(ptr::null_mut(), ptr::null_mut(), 0);
            return;
        }
        // A cached source must not be changed by a later in-place +=. The
        // freshly copied result already has the shared refcount hint.
        js_string_addref(source.cast_mut());
        debug_assert_eq!((*result).refcount, 0);
        store(source.cast_mut(), result, mode);
    }
}

pub(crate) fn scan_trim_cache_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    TRIM_CACHE.with(|cell| unsafe {
        let cache = &mut *cell.get();
        for slot in [&mut cache.source, &mut cache.result] {
            let mut addr = *slot as usize;
            if visitor.visit_tagged_usize_slot(&mut addr, crate::value::STRING_TAG) {
                *slot = addr as *mut StringHeader;
            }
        }
    });
}

#[cfg(test)]
pub(crate) fn test_clear_trim_cache() {
    store(ptr::null_mut(), ptr::null_mut(), 0);
}

#[cfg(test)]
pub(crate) fn test_trim_cache_pair() -> (*mut StringHeader, *mut StringHeader) {
    TRIM_CACHE.with(|cell| unsafe { ((*cell.get()).source, (*cell.get()).result) })
}
