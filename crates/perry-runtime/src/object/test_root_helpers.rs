//! Test-only seeding and inspection helpers for the object realm roots.

use super::*;

#[cfg(test)]
pub(crate) fn test_seed_keys_index_entry(owner: usize) {
    shapes::test_seed_shape_entry(owner);
}

#[cfg(test)]
pub(crate) fn test_keys_index_entry_exists(owner: usize) -> bool {
    shapes::test_shape_entry_exists(owner)
}

#[cfg(test)]
pub(crate) fn test_seed_object_cache_roots(object_cache_bits: [u64; 7], global_this_ptr: i64) {
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    HTTP_METHODS_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(
            slot,
            object_cache_bits[0],
            Ordering::Relaxed,
        );
    });
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    FS_CONSTANTS_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(
            slot,
            object_cache_bits[1],
            Ordering::Relaxed,
        );
    });
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    OS_CONSTANTS_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(
            slot,
            object_cache_bits[2],
            Ordering::Relaxed,
        );
    });
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    OS_CONSTANTS_SIGNALS_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(
            slot,
            object_cache_bits[3],
            Ordering::Relaxed,
        );
    });
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    OS_CONSTANTS_ERRNO_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(
            slot,
            object_cache_bits[4],
            Ordering::Relaxed,
        );
    });
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    OS_CONSTANTS_PRIORITY_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(
            slot,
            object_cache_bits[5],
            Ordering::Relaxed,
        );
    });
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    OS_CONSTANTS_DLOPEN_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(
            slot,
            object_cache_bits[6],
            Ordering::Relaxed,
        );
    });
    // GC_STORE_AUDIT(ROOT): test seed mirrors GLOBAL_THIS_PTR scanned by scan_object_cache_roots_mut.
    crate::gc::runtime_store_root_atomic_raw_i64(
        &GLOBAL_THIS_PTR,
        global_this_ptr,
        Ordering::Release,
    );
    GLOBAL_THIS_READY.store(true, Ordering::Release);
}

#[cfg(test)]
pub(crate) fn test_object_cache_roots() -> ([u64; 7], i64) {
    (
        [
            HTTP_METHODS_CACHE.load(Ordering::Relaxed),
            FS_CONSTANTS_CACHE.load(Ordering::Relaxed),
            OS_CONSTANTS_CACHE.load(Ordering::Relaxed),
            OS_CONSTANTS_SIGNALS_CACHE.load(Ordering::Relaxed),
            OS_CONSTANTS_ERRNO_CACHE.load(Ordering::Relaxed),
            OS_CONSTANTS_PRIORITY_CACHE.load(Ordering::Relaxed),
            OS_CONSTANTS_DLOPEN_CACHE.load(Ordering::Relaxed),
        ],
        GLOBAL_THIS_PTR.load(Ordering::Acquire),
    )
}

/// Materialize every #8002/#8003 realm-owned root on this agent. Kept as one
/// helper so the two-thread isolation gate below cannot accidentally exercise
/// only the backing TLS cells while all builders early-return.
#[cfg(test)]
pub(crate) fn test_materialize_realm_owned_roots() {
    let global = js_get_global_this();
    assert_ne!(
        crate::value::js_nanbox_get_pointer(global),
        0,
        "globalThis bootstrap did not run"
    );
    iterator_prototypes::ensure_iterator_prototypes();
    unsafe {
        let _ = http_methods_array();
        let _ = create_fs_constants_object();
    }
    for (name, cache) in [
        ("os.constants", &OS_CONSTANTS_CACHE),
        ("os.constants.signals", &OS_CONSTANTS_SIGNALS_CACHE),
        ("os.constants.errno", &OS_CONSTANTS_ERRNO_CACHE),
        ("os.constants.priority", &OS_CONSTANTS_PRIORITY_CACHE),
        ("os.constants.dlopen", &OS_CONSTANTS_DLOPEN_CACHE),
    ] {
        let _ = create_cached_sub_namespace(name, cache);
    }
}

/// `(name, backing-atomic address, rooted heap word)` for every root moved by
/// #8002/#8003. The backing address proves the storage is per-agent; the
/// nonzero heap word proves the corresponding builder actually populated it.
#[cfg(test)]
pub(crate) fn test_realm_owned_root_snapshot() -> Vec<(&'static str, usize, u64)> {
    let mut roots = Vec::new();
    for (name, slot) in [
        ("HTTP_METHODS_CACHE", &HTTP_METHODS_CACHE),
        ("FS_CONSTANTS_CACHE", &FS_CONSTANTS_CACHE),
        ("OS_CONSTANTS_CACHE", &OS_CONSTANTS_CACHE),
        ("OS_CONSTANTS_SIGNALS_CACHE", &OS_CONSTANTS_SIGNALS_CACHE),
        ("OS_CONSTANTS_ERRNO_CACHE", &OS_CONSTANTS_ERRNO_CACHE),
        ("OS_CONSTANTS_PRIORITY_CACHE", &OS_CONSTANTS_PRIORITY_CACHE),
        ("OS_CONSTANTS_DLOPEN_CACHE", &OS_CONSTANTS_DLOPEN_CACHE),
    ] {
        roots.push((name, slot.test_slot_addr(), slot.load(Ordering::Acquire)));
    }
    for (name, slot) in [
        ("TYPED_ARRAY_INTRINSIC_PTR", &TYPED_ARRAY_INTRINSIC_PTR),
        (
            "TYPED_ARRAY_INTRINSIC_PROTO_PTR",
            &TYPED_ARRAY_INTRINSIC_PROTO_PTR,
        ),
        (
            "GENERATOR_FUNCTION_INTRINSIC_PTR",
            &GENERATOR_FUNCTION_INTRINSIC_PTR,
        ),
        (
            "GENERATOR_INTRINSIC_PROTO_PTR",
            &GENERATOR_INTRINSIC_PROTO_PTR,
        ),
        ("GENERATOR_PROTOTYPE_PTR", &GENERATOR_PROTOTYPE_PTR),
        (
            "ASYNC_GENERATOR_FUNCTION_INTRINSIC_PTR",
            &ASYNC_GENERATOR_FUNCTION_INTRINSIC_PTR,
        ),
        (
            "ASYNC_GENERATOR_INTRINSIC_PROTO_PTR",
            &ASYNC_GENERATOR_INTRINSIC_PROTO_PTR,
        ),
        (
            "ASYNC_GENERATOR_PROTOTYPE_PTR",
            &ASYNC_GENERATOR_PROTOTYPE_PTR,
        ),
        ("LOCAL_STORAGE_PTR", &LOCAL_STORAGE_PTR),
        ("SESSION_STORAGE_PTR", &SESSION_STORAGE_PTR),
        (
            "ITERATOR_PROTOTYPE_PTR",
            &iterator_prototypes::ITERATOR_PROTOTYPE_PTR,
        ),
        (
            "ARRAY_ITERATOR_PROTOTYPE_PTR",
            &iterator_prototypes::ARRAY_ITERATOR_PROTOTYPE_PTR,
        ),
        (
            "MAP_ITERATOR_PROTOTYPE_PTR",
            &iterator_prototypes::MAP_ITERATOR_PROTOTYPE_PTR,
        ),
        (
            "SET_ITERATOR_PROTOTYPE_PTR",
            &iterator_prototypes::SET_ITERATOR_PROTOTYPE_PTR,
        ),
        (
            "STRING_ITERATOR_PROTOTYPE_PTR",
            &iterator_prototypes::STRING_ITERATOR_PROTOTYPE_PTR,
        ),
        (
            "REGEXP_STRING_ITERATOR_PROTOTYPE_PTR",
            &iterator_prototypes::REGEXP_STRING_ITERATOR_PROTOTYPE_PTR,
        ),
    ] {
        roots.push((
            name,
            slot.test_slot_addr(),
            slot.load(Ordering::Acquire) as u64,
        ));
    }
    global_this::append_async_function_root_snapshot(&mut roots);
    roots
}

#[cfg(test)]
pub(crate) fn test_clear_object_cache_roots() {
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    HTTP_METHODS_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(slot, 0, Ordering::Relaxed);
    });
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    FS_CONSTANTS_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(slot, 0, Ordering::Relaxed);
    });
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    OS_CONSTANTS_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(slot, 0, Ordering::Relaxed);
    });
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    OS_CONSTANTS_SIGNALS_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(slot, 0, Ordering::Relaxed);
    });
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    OS_CONSTANTS_ERRNO_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(slot, 0, Ordering::Relaxed);
    });
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    OS_CONSTANTS_PRIORITY_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(slot, 0, Ordering::Relaxed);
    });
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    OS_CONSTANTS_DLOPEN_CACHE.with_slot(|slot| {
        crate::gc::runtime_store_root_atomic_nanbox_u64(slot, 0, Ordering::Relaxed);
    });
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinel into scanned GLOBAL_THIS_PTR.
    crate::gc::runtime_store_root_atomic_raw_i64(&GLOBAL_THIS_PTR, 0, Ordering::Release);
    GLOBAL_THIS_READY.store(false, Ordering::Release);
}
