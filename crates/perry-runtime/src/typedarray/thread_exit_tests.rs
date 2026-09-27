//! Process-wide admission caches must not outlive a worker's arena.
use super::*;

#[test]
fn thread_exit_invalidates_kind_cache() {
    let address = std::thread::spawn(|| {
        let array = typed_array_alloc(KIND_UINT8, 16);
        let address = array as usize;
        assert_eq!(ta_kind_cache_get(address), Some(Some(KIND_UINT8)));
        address
    })
    .join()
    .unwrap();

    // Probe the cache directly: the registry is thread-local and has gone
    // away, but an old positive cache entry would misclassify a new Promise
    // allocated at the worker's former address (#11463).
    assert_ne!(ta_kind_cache_get(address), Some(Some(KIND_UINT8)));
    assert_eq!(lookup_typed_array_kind(address), None);
}

#[test]
fn thread_exit_invalidates_owning_u32_cache() {
    let address = std::thread::spawn(|| {
        let array = typed_array_alloc(KIND_UINT32, 16);
        let address = array as usize;
        assert_eq!(
            inline_u32_addr(crate::value::js_nanbox_pointer(array as i64)),
            address
        );
        assert!(inline_owning_u32_cache_get(address));
        address
    })
    .join()
    .unwrap();

    // Do not dereference the retired address: only inspect the admission
    // cache that would otherwise let a generated loop use it as a Uint32Array.
    assert!(!inline_owning_u32_cache_get(address));
}

#[test]
fn retiring_range_preserves_live_cache_entries() {
    let retired = typed_array_alloc(KIND_UINT32, 16) as usize;
    // Both caches are direct-mapped. Keep the control in different slots so
    // priming it cannot evict the entry this test intends to invalidate.
    let live = (0..64)
        .map(|_| typed_array_alloc(KIND_UINT32, 16) as usize)
        .find(|&address| {
            ta_kind_cache_slot(address) != ta_kind_cache_slot(retired)
                && inline_owning_u32_cache_slot(address) != inline_owning_u32_cache_slot(retired)
        })
        .expect("distinct admission-cache slots");
    for address in [retired, live] {
        assert_eq!(
            inline_u32_addr(crate::value::js_nanbox_pointer(address as i64)),
            address
        );
        assert_eq!(ta_kind_cache_get(address), Some(Some(KIND_UINT32)));
        assert!(inline_owning_u32_cache_get(address));
    }

    invalidate_caches_in_range(retired, retired + 1);
    assert_eq!(ta_kind_cache_get(retired), None);
    assert!(!inline_owning_u32_cache_get(retired));
    assert_eq!(ta_kind_cache_get(live), Some(Some(KIND_UINT32)));
    assert!(inline_owning_u32_cache_get(live));
}
