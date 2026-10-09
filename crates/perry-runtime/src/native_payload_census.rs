//! Scalar-only acceptance census for net.Socket payloads.
use std::sync::atomic::{AtomicU64, Ordering};
static CREATED: AtomicU64 = AtomicU64::new(0);
static FINALIZED: AtomicU64 = AtomicU64::new(0);
per_test_global! {
    static DROPS: AtomicU64 = AtomicU64::new(0);
}
static REFS: AtomicU64 = AtomicU64::new(0);
fn counted(tag: u64) -> bool {
    tag as u32 == crate::native_class_ids::NET_SOCKET
}
/// `[created, finalized, payload drops, outstanding cell refs]`.
pub fn snapshot() -> [u64; 4] {
    [
        CREATED.load(Ordering::SeqCst),
        FINALIZED.load(Ordering::SeqCst),
        DROPS.load(Ordering::SeqCst),
        REFS.load(Ordering::SeqCst),
    ]
}
pub(crate) fn created(tag: u64) {
    if counted(tag) {
        CREATED.fetch_add(1, Ordering::SeqCst);
    }
}
pub(crate) fn finalized(tag: u64, refs: u32) {
    if counted(tag) {
        FINALIZED.fetch_add(1, Ordering::SeqCst);
        // Completions discarded at worker teardown never unref; the heap
        // retires their remaining refs with the cell.
        REFS.fetch_sub(refs as u64, Ordering::SeqCst);
    }
}
pub(crate) fn dropped(tag: u64) {
    if counted(tag) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}
pub(crate) fn reference(tag: u64) {
    if counted(tag) {
        REFS.fetch_add(1, Ordering::SeqCst);
    }
}
pub(crate) fn unreference(tag: u64) {
    if counted(tag) {
        REFS.fetch_sub(1, Ordering::SeqCst);
    }
}
