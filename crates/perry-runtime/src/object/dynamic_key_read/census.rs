//! Diagnostic counts only: how many computed-read keys were atoms, inline
//! (SSO) strings, non-atom heap strings or other values. Plain counters; no
//! key, object or site is retained.
use std::sync::atomic::{AtomicU64, Ordering};

per_test_global! {
    static ATOM_KEYS: AtomicU64 = AtomicU64::new(0);
}
per_test_global! {
    static SSO_KEYS: AtomicU64 = AtomicU64::new(0);
}
per_test_global! {
    static HEAP_KEYS: AtomicU64 = AtomicU64::new(0);
}
per_test_global! {
    static OTHER_KEYS: AtomicU64 = AtomicU64::new(0);
}

#[inline]
pub(super) fn record(key: u64) {
    if !crate::object::method_site::stats_report_enabled() {
        return;
    }
    let kind = unsafe {
        match key >> 48 {
            0x7FF9 => 1,
            0x7FFF => {
                let ptr = (key & crate::value::POINTER_MASK) as *const crate::StringHeader;
                let bytes = std::slice::from_raw_parts(
                    crate::string::string_data(ptr),
                    (*ptr).byte_len as usize,
                );
                let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
                if crate::string::atom_lookup(bytes, hash) == Some(ptr) {
                    0
                } else {
                    2
                }
            }
            _ => 3,
        }
    };
    match kind {
        0 => ATOM_KEYS.fetch_add(1, Ordering::Relaxed),
        1 => SSO_KEYS.fetch_add(1, Ordering::Relaxed),
        2 => HEAP_KEYS.fetch_add(1, Ordering::Relaxed),
        _ => OTHER_KEYS.fetch_add(1, Ordering::Relaxed),
    };
}

pub(crate) fn report() {
    eprintln!(
        "[key-census] atom={} sso={} heap={} other={}",
        ATOM_KEYS.load(Ordering::Relaxed),
        SSO_KEYS.load(Ordering::Relaxed),
        HEAP_KEYS.load(Ordering::Relaxed),
        OTHER_KEYS.load(Ordering::Relaxed),
    );
}
