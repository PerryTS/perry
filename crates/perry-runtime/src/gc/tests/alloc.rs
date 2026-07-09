use super::super::*;
use super::support::*;

fn reset_malloc_kind_telemetry_for_tests() {
    MALLOC_STATE.with(|s| {
        let mut s = s.borrow_mut();
        let mut telemetry = [MallocKindTelemetry::zero(); MALLOC_KIND_BUCKET_COUNT];
        for &header in s.objects.iter() {
            unsafe {
                let counters = &mut telemetry[malloc_kind_index((*header).obj_type)];
                counters.survivor_count = counters.survivor_count.saturating_add(1);
                counters.survivor_bytes = counters
                    .survivor_bytes
                    .saturating_add((*header).size as u64);
            }
        }
        s.kind_telemetry = telemetry;
    });
}

fn malloc_kind_telemetry_for_tests(obj_type: u8) -> MallocKindTelemetry {
    MALLOC_STATE.with(|s| s.borrow().kind_telemetry[malloc_kind_index(obj_type)])
}

fn mark_existing_malloc_and_arena_objects_except(excluded: &[usize]) {
    MALLOC_STATE.with(|s| {
        for &tracked in s.borrow().objects.iter() {
            if !excluded.contains(&(tracked as usize)) {
                unsafe {
                    (*tracked).gc_flags |= GC_FLAG_MARKED;
                }
            }
        }
    });
    crate::arena::arena_walk_objects(|arena_header| unsafe {
        (*(arena_header as *mut GcHeader)).gc_flags |= GC_FLAG_MARKED;
    });
}

#[test]
fn test_gc_malloc_basic() {
    // Allocate a string-type object
    let ptr = gc_malloc(64, GC_TYPE_STRING);
    assert!(!ptr.is_null());

    // Verify header is set correctly
    unsafe {
        let header = header_from_user_ptr(ptr);
        assert_eq!((*header).obj_type, GC_TYPE_STRING);
        assert_eq!((*header).gc_flags, 0); // not arena, not marked
        assert_eq!((*header).size as usize, GC_HEADER_SIZE + 64);
    }

    // Verify it's tracked in MALLOC_OBJECTS (rebuild lazy set first)
    let tracked = MALLOC_STATE.with(|s| {
        let header = unsafe { header_from_user_ptr(ptr) };
        let mut s = s.borrow_mut();
        ensure_set_built(&mut s);
        s.set.contains(&(header as usize))
    });
    assert!(tracked, "allocated object should be tracked in MALLOC_SET");
}

#[test]
fn test_gc_malloc_different_types() {
    let string_ptr = gc_malloc(32, GC_TYPE_STRING);
    let closure_ptr = gc_malloc(48, GC_TYPE_CLOSURE);
    let bigint_ptr = gc_malloc(16, GC_TYPE_BIGINT);

    unsafe {
        init_test_closure(closure_ptr);
        assert_eq!((*header_from_user_ptr(string_ptr)).obj_type, GC_TYPE_STRING);
        assert_eq!(
            (*header_from_user_ptr(closure_ptr)).obj_type,
            GC_TYPE_CLOSURE
        );
        assert_eq!((*header_from_user_ptr(bigint_ptr)).obj_type, GC_TYPE_BIGINT);
    }
}

#[test]
fn test_sweep_removes_unmarked_malloc_object() {
    let ptr = gc_malloc(64, GC_TYPE_STRING);
    let header = unsafe { header_from_user_ptr(ptr) };
    let header_addr = header as usize;

    let tracked_before = MALLOC_STATE.with(|s| {
        s.borrow()
            .objects
            .iter()
            .any(|&tracked| tracked as usize == header_addr)
    });
    assert!(
        tracked_before,
        "new gc_malloc object should be tracked before sweep"
    );

    // Direct sweep is intentionally rootless for this regression. Keep
    // older test allocations marked so this assertion is about only the
    // object created above.
    MALLOC_STATE.with(|s| {
        for &tracked in s.borrow().objects.iter() {
            if tracked as usize != header_addr {
                unsafe {
                    (*tracked).gc_flags |= GC_FLAG_MARKED;
                }
            }
        }
    });
    crate::arena::arena_walk_objects(|arena_header| unsafe {
        (*(arena_header as *mut GcHeader)).gc_flags |= GC_FLAG_MARKED;
    });

    let freed = sweep();
    assert!(
        freed >= (GC_HEADER_SIZE + 64) as u64,
        "sweep should report at least the target malloc object as freed"
    );

    let tracked_after = MALLOC_STATE.with(|s| {
        s.borrow()
            .objects
            .iter()
            .any(|&tracked| tracked as usize == header_addr)
    });
    assert!(
        !tracked_after,
        "unmarked malloc object should be removed from MALLOC_STATE.objects"
    );

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_gc_collect_updates_stats() {
    // Get initial stats
    let initial_count = GC_STATS.with(|s| s.borrow().collection_count);

    // Run GC
    gc_collect_inner();

    // Stats should have incremented
    let new_count = GC_STATS.with(|s| s.borrow().collection_count);
    assert_eq!(
        new_count,
        initial_count + 1,
        "collection count should increment"
    );
}

#[test]
fn test_gc_header_size() {
    assert_eq!(GC_HEADER_SIZE, 8, "GC header should be 8 bytes");
}

/// Issue #179: block-persist's age window must match the reset
/// policy's `keep_low` window — both define the set of blocks
/// where caller-saved-register handles might still be uncaptured.
/// If the two drift apart, block-persist either over-retains old
/// blocks (RSS regression) or under-protects recent blocks
/// (re-opens the issues #43 / #44 dangling-pointer failure mode).
#[test]
fn block_persist_window_matches_reset_keep_low() {
    // `keep_low = current.saturating_sub(4)` → 5 blocks
    // (current-4..=current). `BLOCK_PERSIST_WINDOW` gates Pass 2
    // of `mark_block_persisting_arena_objects` via
    // `persist_low = general_n.saturating_sub(BLOCK_PERSIST_WINDOW)`.
    // Both windows must describe the same "register-miss risk"
    // horizon for the correctness invariant to hold.
    assert_eq!(
        BLOCK_PERSIST_WINDOW, 5,
        "block-persist window must match reset's keep_low window (5 blocks)"
    );
}

/// Issue #179: `gc_collect_inner` must return the sweep's
/// freed_bytes so the adaptive step logic can react to
/// object-reclaim activity immediately, not wait for blocks to
/// clear the 2-cycle grace and surface as a `pre - post` drop on
/// the next cycle. The return value drives the `>90% halve /
/// 10-90% halve / <10% double` classifier in `gc_check_trigger`.
#[test]
fn gc_collect_inner_returns_freed_bytes() {
    // Allocate an object that's guaranteed unreachable (no
    // roots hold it — we immediately drop the pointer).
    let _throwaway = gc_malloc(128, GC_TYPE_STRING);
    // freed_bytes is the per-sweep reclaim count; for this
    // tiny test we just assert the signature (returns u64).
    // The exact freed count depends on thread-local state from
    // other tests, so we only assert the type/shape.
    let _freed: u64 = gc_collect_inner();
}

#[test]
fn test_gc_realloc_basic() {
    let ptr = gc_malloc(32, GC_TYPE_STRING);
    assert!(!ptr.is_null());

    // Write some data
    unsafe {
        std::ptr::write_bytes(ptr, 0xAB, 32);
    }

    // Reallocate to larger size
    let new_ptr = gc_realloc(ptr, 128);
    assert!(!new_ptr.is_null());

    // Verify old data preserved (first 32 bytes should still be 0xAB)
    unsafe {
        for i in 0..32 {
            assert_eq!(
                *new_ptr.add(i),
                0xAB,
                "byte {} should be preserved after realloc",
                i
            );
        }
    }

    // Verify tracking updated (rebuild lazy set first)
    let tracked = MALLOC_STATE.with(|s| {
        let header = unsafe { header_from_user_ptr(new_ptr) };
        let mut s = s.borrow_mut();
        ensure_set_built(&mut s);
        s.set.contains(&(header as usize))
    });
    assert!(tracked, "reallocated object should be tracked");
}

#[test]
fn test_gc_realloc_null_allocates_fresh() {
    let ptr = gc_realloc(std::ptr::null_mut(), 64);
    assert!(!ptr.is_null(), "realloc(null) should allocate fresh");
}

#[test]
fn test_gc_mark_flags() {
    let ptr = gc_malloc(32, GC_TYPE_STRING);
    unsafe {
        let header = header_from_user_ptr(ptr);

        // Initially not marked
        assert_eq!((*header).gc_flags & GC_FLAG_MARKED, 0);

        // Mark it
        (*header).gc_flags |= GC_FLAG_MARKED;
        assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);

        // Clear mark
        (*header).gc_flags &= !GC_FLAG_MARKED;
        assert_eq!((*header).gc_flags & GC_FLAG_MARKED, 0);
    }
}

#[test]
fn test_gc_pinned_flag() {
    let ptr = gc_malloc(32, GC_TYPE_STRING);
    unsafe {
        let header = header_from_user_ptr(ptr);

        // Pin it
        (*header).gc_flags |= GC_FLAG_PINNED;

        // Run GC - pinned objects should survive
        gc_collect_inner();

        // Verify still tracked (rebuild lazy set first)
        let tracked = MALLOC_STATE.with(|s| {
            let mut s = s.borrow_mut();
            ensure_set_built(&mut s);
            s.set.contains(&(header as usize))
        });
        assert!(tracked, "pinned object should survive GC");

        // Unpin
        (*header).gc_flags &= !GC_FLAG_PINNED;
    }
}

#[test]
fn test_build_valid_pointer_set() {
    // Allocate some objects
    let ptr1 = gc_malloc(32, GC_TYPE_STRING);
    let ptr2 = gc_malloc(64, GC_TYPE_CLOSURE);
    unsafe {
        init_test_closure(ptr2);
    }

    let valid_set = build_valid_pointer_set();

    // Our malloc objects should be in the valid set
    assert!(
        valid_set.contains(&(ptr1 as usize)),
        "ptr1 should be in valid set"
    );
    assert!(
        valid_set.contains(&(ptr2 as usize)),
        "ptr2 should be in valid set"
    );
}

#[test]
fn test_gc_type_metadata_covers_all_declared_types() {
    let declared_types = include_str!("../types.rs")
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("pub const GC_TYPE_")?;
            let (name, value) = rest.split_once(": u8 = ")?;
            if name == "MAX" {
                return None;
            }
            let value = value.trim_end_matches(';').parse::<u8>().ok()?;
            Some((name, value))
        })
        .collect::<Vec<_>>();
    assert_eq!(declared_types.len(), GC_TYPE_MAX as usize);
    for &(name, type_id) in &declared_types {
        assert!(
            gc_type_info(type_id).is_some(),
            "missing metadata for declared GC_TYPE_{name}={type_id}"
        );
    }

    let infos = gc_type_infos().collect::<Vec<_>>();
    assert_eq!(infos.len(), GC_TYPE_MAX as usize);

    let mut seen = [false; MALLOC_KIND_BUCKET_COUNT];
    for info in infos {
        assert_ne!(info.type_id, 0, "unknown is not a declared GC type");
        assert!(
            (info.type_id as usize) < MALLOC_KIND_BUCKET_COUNT,
            "metadata type id out of range: {}",
            info.type_id
        );
        assert!(
            !seen[info.type_id as usize],
            "duplicate metadata for {}",
            info.name
        );
        seen[info.type_id as usize] = true;
        assert_eq!(gc_type_info(info.type_id).copied(), Some(*info));
        assert_eq!(gc_type_name(info.type_id), info.name);
    }

    for type_id in 1..MALLOC_KIND_BUCKET_COUNT {
        assert!(seen[type_id], "missing metadata for GC type {type_id}");
    }
    validate_gc_type_metadata().expect("declared GC type metadata should be internally valid");

    let expected_infos = [
        GcTypeInfo {
            type_id: GC_TYPE_ARRAY,
            name: "array",
            allocation_policy: GcAllocationPolicy::Arena,
            arena_walkable: true,
            rewrite_descriptor_kind: GcRewriteDescriptorKind::Array,
            layout_slot_kind: GcLayoutSlotKind::ArrayElements,
            movable: true,
            external_byte_policy: GcExternalBytePolicy::InlinePayload,
            large_object_policy: GcLargeObjectPolicy::OldArenaWhenOverThreshold,
            pointer_free: false,
            move_hook_kind: GcMoveHookKind::None,
            rewrite_hook_kind: GcRewriteHookKind::None,
            finalize_hook_kind: GcFinalizeHookKind::None,
        },
        GcTypeInfo {
            type_id: GC_TYPE_OBJECT,
            name: "object",
            allocation_policy: GcAllocationPolicy::ArenaOrMalloc,
            arena_walkable: true,
            rewrite_descriptor_kind: GcRewriteDescriptorKind::Object,
            layout_slot_kind: GcLayoutSlotKind::ObjectFields,
            movable: true,
            external_byte_policy: GcExternalBytePolicy::InlinePayload,
            large_object_policy: GcLargeObjectPolicy::OldArenaWhenOverThreshold,
            pointer_free: false,
            move_hook_kind: GcMoveHookKind::ObjectOverflowFields,
            rewrite_hook_kind: GcRewriteHookKind::None,
            finalize_hook_kind: GcFinalizeHookKind::None,
        },
        GcTypeInfo {
            type_id: GC_TYPE_STRING,
            name: "string",
            allocation_policy: GcAllocationPolicy::ArenaOrMalloc,
            arena_walkable: true,
            rewrite_descriptor_kind: GcRewriteDescriptorKind::Leaf,
            layout_slot_kind: GcLayoutSlotKind::None,
            movable: true,
            external_byte_policy: GcExternalBytePolicy::InlinePayload,
            large_object_policy: GcLargeObjectPolicy::OldArenaWhenOverThreshold,
            pointer_free: true,
            move_hook_kind: GcMoveHookKind::None,
            rewrite_hook_kind: GcRewriteHookKind::None,
            finalize_hook_kind: GcFinalizeHookKind::None,
        },
        GcTypeInfo {
            type_id: GC_TYPE_CLOSURE,
            name: "closure",
            allocation_policy: GcAllocationPolicy::ArenaOrMalloc,
            arena_walkable: true,
            rewrite_descriptor_kind: GcRewriteDescriptorKind::Closure,
            layout_slot_kind: GcLayoutSlotKind::ClosureCaptures,
            movable: true,
            external_byte_policy: GcExternalBytePolicy::InlinePayload,
            large_object_policy: GcLargeObjectPolicy::MallocTracked,
            pointer_free: false,
            move_hook_kind: GcMoveHookKind::ClosureDynamicProps,
            rewrite_hook_kind: GcRewriteHookKind::None,
            finalize_hook_kind: GcFinalizeHookKind::None,
        },
        GcTypeInfo {
            type_id: GC_TYPE_PROMISE,
            name: "promise",
            allocation_policy: GcAllocationPolicy::ArenaOrMalloc,
            arena_walkable: true,
            rewrite_descriptor_kind: GcRewriteDescriptorKind::Promise,
            layout_slot_kind: GcLayoutSlotKind::None,
            movable: true,
            external_byte_policy: GcExternalBytePolicy::None,
            large_object_policy: GcLargeObjectPolicy::MallocTracked,
            pointer_free: false,
            // #5142: rekey address-keyed promise expandos on GC move.
            move_hook_kind: GcMoveHookKind::ExoticExpandoOwner,
            rewrite_hook_kind: GcRewriteHookKind::None,
            finalize_hook_kind: GcFinalizeHookKind::PromiseCleanup,
        },
        GcTypeInfo {
            type_id: GC_TYPE_BIGINT,
            name: "bigint",
            allocation_policy: GcAllocationPolicy::ArenaOrMalloc,
            arena_walkable: true,
            rewrite_descriptor_kind: GcRewriteDescriptorKind::Leaf,
            layout_slot_kind: GcLayoutSlotKind::None,
            movable: true,
            external_byte_policy: GcExternalBytePolicy::InlinePayload,
            large_object_policy: GcLargeObjectPolicy::OldArenaWhenOverThreshold,
            pointer_free: true,
            move_hook_kind: GcMoveHookKind::None,
            rewrite_hook_kind: GcRewriteHookKind::None,
            finalize_hook_kind: GcFinalizeHookKind::None,
        },
        GcTypeInfo {
            type_id: GC_TYPE_ERROR,
            name: "error",
            allocation_policy: GcAllocationPolicy::Arena,
            arena_walkable: true,
            rewrite_descriptor_kind: GcRewriteDescriptorKind::Error,
            layout_slot_kind: GcLayoutSlotKind::None,
            movable: true,
            external_byte_policy: GcExternalBytePolicy::None,
            large_object_policy: GcLargeObjectPolicy::OldArenaWhenOverThreshold,
            pointer_free: false,
            move_hook_kind: GcMoveHookKind::ErrorSideTables,
            rewrite_hook_kind: GcRewriteHookKind::None,
            finalize_hook_kind: GcFinalizeHookKind::ErrorSideTables,
        },
        GcTypeInfo {
            type_id: GC_TYPE_MAP,
            name: "map",
            allocation_policy: GcAllocationPolicy::Arena,
            arena_walkable: true,
            rewrite_descriptor_kind: GcRewriteDescriptorKind::Map,
            layout_slot_kind: GcLayoutSlotKind::None,
            movable: true,
            external_byte_policy: GcExternalBytePolicy::SideAllocation,
            large_object_policy: GcLargeObjectPolicy::NotApplicable,
            pointer_free: false,
            move_hook_kind: GcMoveHookKind::MapSideTables,
            rewrite_hook_kind: GcRewriteHookKind::None,
            finalize_hook_kind: GcFinalizeHookKind::MapSideAllocation,
        },
        GcTypeInfo {
            type_id: GC_TYPE_LAZY_ARRAY,
            name: "lazy_array",
            allocation_policy: GcAllocationPolicy::Arena,
            arena_walkable: true,
            rewrite_descriptor_kind: GcRewriteDescriptorKind::LazyArray,
            layout_slot_kind: GcLayoutSlotKind::None,
            movable: true,
            external_byte_policy: GcExternalBytePolicy::InlinePayload,
            large_object_policy: GcLargeObjectPolicy::OldArenaWhenOverThreshold,
            pointer_free: false,
            move_hook_kind: GcMoveHookKind::None,
            rewrite_hook_kind: GcRewriteHookKind::None,
            finalize_hook_kind: GcFinalizeHookKind::None,
        },
        GcTypeInfo {
            type_id: GC_TYPE_BUFFER,
            name: "buffer",
            allocation_policy: GcAllocationPolicy::RawOrLargeOldArena,
            arena_walkable: true,
            rewrite_descriptor_kind: GcRewriteDescriptorKind::Leaf,
            layout_slot_kind: GcLayoutSlotKind::None,
            movable: false,
            external_byte_policy: GcExternalBytePolicy::InlinePayload,
            large_object_policy: GcLargeObjectPolicy::OldArenaWhenOverThreshold,
            pointer_free: true,
            move_hook_kind: GcMoveHookKind::None,
            rewrite_hook_kind: GcRewriteHookKind::None,
            finalize_hook_kind: GcFinalizeHookKind::None,
        },
        GcTypeInfo {
            type_id: GC_TYPE_TYPED_ARRAY,
            name: "typed_array",
            allocation_policy: GcAllocationPolicy::RawOrLargeOldArena,
            arena_walkable: true,
            rewrite_descriptor_kind: GcRewriteDescriptorKind::Leaf,
            layout_slot_kind: GcLayoutSlotKind::None,
            movable: false,
            external_byte_policy: GcExternalBytePolicy::InlinePayload,
            large_object_policy: GcLargeObjectPolicy::OldArenaWhenOverThreshold,
            pointer_free: true,
            move_hook_kind: GcMoveHookKind::None,
            rewrite_hook_kind: GcRewriteHookKind::None,
            finalize_hook_kind: GcFinalizeHookKind::None,
        },
        GcTypeInfo {
            type_id: GC_TYPE_SET,
            name: "set",
            allocation_policy: GcAllocationPolicy::Arena,
            arena_walkable: true,
            rewrite_descriptor_kind: GcRewriteDescriptorKind::Set,
            layout_slot_kind: GcLayoutSlotKind::None,
            movable: true,
            external_byte_policy: GcExternalBytePolicy::SideAllocation,
            large_object_policy: GcLargeObjectPolicy::NotApplicable,
            pointer_free: false,
            move_hook_kind: GcMoveHookKind::SetSideTables,
            rewrite_hook_kind: GcRewriteHookKind::SetIndex,
            finalize_hook_kind: GcFinalizeHookKind::SetSideAllocation,
        },
    ];

    for expected in expected_infos {
        let info = gc_type_info(expected.type_id)
            .copied()
            .unwrap_or_else(|| panic!("missing metadata for {}", expected.name));
        assert_eq!(info, expected, "metadata mismatch for {}", expected.name);
        assert_eq!(gc_type_name(expected.type_id), expected.name);
        assert_eq!(
            gc_type_rewrite_descriptor_kind(expected.type_id),
            expected.rewrite_descriptor_kind
        );
        assert_eq!(
            gc_type_layout_slot_kind(expected.type_id),
            expected.layout_slot_kind
        );
        assert_eq!(
            gc_type_external_byte_policy(expected.type_id),
            expected.external_byte_policy
        );
        assert_eq!(
            gc_type_large_object_policy(expected.type_id),
            expected.large_object_policy
        );
        assert_eq!(
            gc_type_is_pointer_free(expected.type_id),
            expected.pointer_free
        );
        assert_eq!(gc_type_is_movable(expected.type_id), expected.movable);
        assert_eq!(
            gc_type_is_arena_walkable(expected.type_id),
            expected.arena_walkable
        );
    }

    assert!(!gc_type_is_arena_walkable(0));
    assert!(!gc_type_is_movable(0));
    assert_eq!(
        gc_type_rewrite_descriptor_kind(0),
        GcRewriteDescriptorKind::Leaf
    );
    assert_eq!(gc_type_layout_slot_kind(0), GcLayoutSlotKind::None);
    assert_eq!(gc_type_external_byte_policy(0), GcExternalBytePolicy::None);
    assert_eq!(
        gc_type_large_object_policy(0),
        GcLargeObjectPolicy::NotApplicable
    );
    assert!(gc_type_is_pointer_free(0));
}

#[test]
fn test_gc_type_metadata_verifier_rejects_invalid_pointer_contracts() {
    let mut missing_descriptor = *gc_type_info(GC_TYPE_OBJECT).unwrap();
    missing_descriptor.rewrite_descriptor_kind = GcRewriteDescriptorKind::Leaf;
    assert!(validate_gc_type_info(&missing_descriptor).is_err());

    let mut pointer_free_with_slots = *gc_type_info(GC_TYPE_STRING).unwrap();
    pointer_free_with_slots.layout_slot_kind = GcLayoutSlotKind::ObjectFields;
    assert!(validate_gc_type_info(&pointer_free_with_slots).is_err());

    let mut pointer_free_with_descriptor = *gc_type_info(GC_TYPE_STRING).unwrap();
    pointer_free_with_descriptor.rewrite_descriptor_kind = GcRewriteDescriptorKind::Object;
    assert!(validate_gc_type_info(&pointer_free_with_descriptor).is_err());
}

fn malloc_kind_test_payload_size(obj_type: u8) -> usize {
    match obj_type {
        GC_TYPE_STRING => std::mem::size_of::<crate::string::StringHeader>() + 8,
        GC_TYPE_CLOSURE => std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_PROMISE => std::mem::size_of::<crate::promise::Promise>(),
        GC_TYPE_BIGINT => std::mem::size_of::<crate::bigint::BigIntHeader>(),
        GC_TYPE_ERROR => std::mem::size_of::<crate::error::ErrorHeader>(),
        _ => 16,
    }
}

fn alloc_malloc_kind_test_object(obj_type: u8) -> *mut u8 {
    let ptr = gc_malloc(malloc_kind_test_payload_size(obj_type), obj_type);
    unsafe {
        match obj_type {
            GC_TYPE_STRING => {
                std::ptr::write(
                    ptr as *mut crate::string::StringHeader,
                    crate::string::StringHeader {
                        utf16_len: 0,
                        byte_len: 0,
                        capacity: 8,
                        refcount: 0,
                        flags: 0,
                    },
                );
            }
            GC_TYPE_CLOSURE => init_test_closure(ptr),
            GC_TYPE_PROMISE => {
                std::ptr::write(
                    ptr as *mut crate::promise::Promise,
                    crate::promise::Promise {
                        state: crate::promise::PromiseState::Pending,
                        value: 0.0,
                        reason: 0.0,
                        on_fulfilled: std::ptr::null(),
                        on_rejected: std::ptr::null(),
                        next: std::ptr::null_mut(),
                        async_id: 0,
                        trigger_async_id: 0,
                    },
                );
            }
            GC_TYPE_BIGINT => {
                std::ptr::write(
                    ptr as *mut crate::bigint::BigIntHeader,
                    crate::bigint::BigIntHeader {
                        limbs: [0; crate::bigint::BIGINT_LIMBS],
                    },
                );
            }
            GC_TYPE_ERROR => {
                std::ptr::write(
                    ptr as *mut crate::error::ErrorHeader,
                    crate::error::ErrorHeader {
                        object_type: crate::error::OBJECT_TYPE_ERROR,
                        error_kind: crate::error::ERROR_KIND_ERROR,
                        flags: 0,
                        message: std::ptr::null_mut(),
                        name: std::ptr::null_mut(),
                        stack: std::ptr::null_mut(),
                        cause: 0.0,
                        errors: std::ptr::null_mut(),
                    },
                );
            }
            _ => {}
        }
    }
    ptr
}

#[test]
fn test_small_js_string_alloc_uses_managed_nursery_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let string = crate::string::js_string_from_bytes(b"managed-string".as_ptr(), 14);
    let header = unsafe { header_from_user_ptr(string as *const u8) };

    unsafe {
        assert_eq!((*header).obj_type, GC_TYPE_STRING);
        assert_ne!((*header).gc_flags & GC_FLAG_ARENA, 0);
    }
    assert_eq!(
        crate::arena::classify_heap_generation(string as usize),
        crate::arena::HeapGeneration::Nursery
    );
    assert!(
        !malloc_user_ptr_tracked(string as *mut u8),
        "ordinary heap strings should not be tracked in MALLOC_STATE"
    );
}

#[test]
fn test_small_js_closure_alloc_uses_managed_nursery_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let closure = crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 2);
    let header = unsafe { header_from_user_ptr(closure as *const u8) };

    unsafe {
        assert_eq!((*header).obj_type, GC_TYPE_CLOSURE);
        assert_ne!((*header).gc_flags & GC_FLAG_ARENA, 0);
    }
    assert_eq!(
        crate::arena::classify_heap_generation(closure as usize),
        crate::arena::HeapGeneration::Nursery
    );
    assert!(
        !malloc_user_ptr_tracked(closure as *mut u8),
        "ordinary closures should not be tracked in MALLOC_STATE"
    );
}

#[test]
fn test_small_map_alloc_uses_managed_nursery_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let map = crate::map::js_map_alloc(4);
    let header = unsafe { header_from_user_ptr(map as *const u8) };

    unsafe {
        assert_eq!((*header).obj_type, GC_TYPE_MAP);
        assert_ne!((*header).gc_flags & GC_FLAG_ARENA, 0);
    }
    assert_eq!(
        crate::arena::classify_heap_generation(map as usize),
        crate::arena::HeapGeneration::Nursery
    );
    assert!(
        !malloc_user_ptr_tracked(map as *mut u8),
        "ordinary Map headers should not be tracked in MALLOC_STATE"
    );
    assert!(crate::map::is_registered_map(map as usize));
}

#[test]
fn test_small_set_alloc_uses_managed_nursery_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let set = crate::set::js_set_alloc(4);
    let header = unsafe { header_from_user_ptr(set as *const u8) };

    unsafe {
        assert_eq!((*header).obj_type, GC_TYPE_SET);
        assert_ne!((*header).gc_flags & GC_FLAG_ARENA, 0);
    }
    assert_eq!(
        crate::arena::classify_heap_generation(set as usize),
        crate::arena::HeapGeneration::Nursery
    );
    assert!(
        !malloc_user_ptr_tracked(set as *mut u8),
        "ordinary Set headers should not be tracked in MALLOC_STATE"
    );
    assert!(crate::set::is_registered_set(set as usize));
}

#[test]
fn test_map_set_churn_no_malloc_kind_telemetry() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_malloc_kind_telemetry_for_tests();
    let map_before = malloc_kind_telemetry_for_tests(GC_TYPE_MAP);
    let set_before = malloc_kind_telemetry_for_tests(GC_TYPE_SET);

    for i in 0..64 {
        let map = crate::map::js_map_alloc(4);
        crate::map::js_map_set(map, i as f64, (i * 2) as f64);
        let set = crate::set::js_set_alloc(4);
        crate::set::js_set_add(set, i as f64);
    }

    let map_after = malloc_kind_telemetry_for_tests(GC_TYPE_MAP);
    let set_after = malloc_kind_telemetry_for_tests(GC_TYPE_SET);
    assert_eq!(map_after.allocated_count, map_before.allocated_count);
    assert_eq!(map_after.allocated_bytes, map_before.allocated_bytes);
    assert_eq!(map_after.survivor_count, map_before.survivor_count);
    assert_eq!(map_after.survivor_bytes, map_before.survivor_bytes);
    assert_eq!(set_after.allocated_count, set_before.allocated_count);
    assert_eq!(set_after.allocated_bytes, set_before.allocated_bytes);
    assert_eq!(set_after.survivor_count, set_before.survivor_count);
    assert_eq!(set_after.survivor_bytes, set_before.survivor_bytes);
}

#[test]
fn test_large_js_closure_alloc_remains_malloc_tracked() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let max_managed_captures = (LARGE_OBJECT_THRESHOLD_BYTES
        - GC_HEADER_SIZE
        - std::mem::size_of::<crate::closure::ClosureHeader>())
        / std::mem::size_of::<u64>();
    let closure = crate::closure::js_closure_alloc(
        test_captured_singleton_func as *const u8,
        (max_managed_captures + 1) as u32,
    );
    let header = unsafe { header_from_user_ptr(closure as *const u8) };

    unsafe {
        assert_eq!((*header).obj_type, GC_TYPE_CLOSURE);
        assert_eq!((*header).gc_flags & GC_FLAG_ARENA, 0);
    }
    assert!(
        malloc_user_ptr_tracked(closure as *mut u8),
        "large closure environments should keep the explicit gc_malloc path"
    );
}

#[test]
fn test_ordinary_promise_alloc_uses_managed_nursery_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    crate::async_hooks::reset_for_tests();
    let promise = crate::promise::js_promise_new();
    let header = unsafe { header_from_user_ptr(promise as *const u8) };

    unsafe {
        assert_eq!((*header).obj_type, GC_TYPE_PROMISE);
        assert_ne!((*header).gc_flags & GC_FLAG_ARENA, 0);
    }
    assert_eq!(
        crate::arena::classify_heap_generation(promise as usize),
        crate::arena::HeapGeneration::Nursery
    );
    assert!(
        !malloc_user_ptr_tracked(promise as *mut u8),
        "ordinary promises should not be tracked in MALLOC_STATE"
    );
}

#[test]
fn test_async_hooks_promise_alloc_remains_malloc_tracked() {
    crate::async_hooks::reset_for_tests();
    crate::async_hooks::test_seed_async_hooks_scanner_roots(std::ptr::null(), 0.0);
    let promise = crate::promise::js_promise_new();
    let header = unsafe { header_from_user_ptr(promise as *const u8) };

    unsafe {
        assert_eq!((*header).obj_type, GC_TYPE_PROMISE);
        assert_eq!((*header).gc_flags & GC_FLAG_ARENA, 0);
        assert_ne!((*promise).async_id, 0);
    }
    assert!(
        malloc_user_ptr_tracked(promise as *mut u8),
        "async_hooks promises keep the explicit gc_malloc path"
    );
    crate::async_hooks::reset_for_tests();
}

#[test]
fn test_builtin_error_alloc_uses_managed_nursery_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let message = crate::string::js_string_from_bytes(b"managed error".as_ptr(), 13);
    let error = crate::error::js_error_new_with_message(message);
    let header = unsafe { header_from_user_ptr(error as *const u8) };

    unsafe {
        assert_eq!((*header).obj_type, GC_TYPE_ERROR);
        assert_ne!((*header).gc_flags & GC_FLAG_ARENA, 0);
        assert_eq!((*error).message, message);
    }
    assert_eq!(
        crate::arena::classify_heap_generation(error as usize),
        crate::arena::HeapGeneration::Nursery
    );
    assert!(
        !malloc_user_ptr_tracked(error as *mut u8),
        "built-in errors should not be tracked in MALLOC_STATE"
    );
}

#[test]
fn test_thread_bigint_deserialization_uses_managed_nursery_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let mut limbs = [0; crate::bigint::BIGINT_LIMBS];
    limbs[0] = 42;
    let bits = unsafe { crate::thread::test_deserialize_bigint_limbs(limbs) };
    assert_eq!(bits & TAG_MASK, BIGINT_TAG);
    let ptr = (bits & POINTER_MASK) as *mut crate::bigint::BigIntHeader;
    let header = unsafe { header_from_user_ptr(ptr as *const u8) };

    unsafe {
        assert_eq!((*header).obj_type, GC_TYPE_BIGINT);
        assert_ne!((*header).gc_flags & GC_FLAG_ARENA, 0);
        assert_eq!((*ptr).limbs[0], 42);
    }
    assert!(
        !malloc_user_ptr_tracked(ptr as *mut u8),
        "thread BigInt deserialization should reuse the arena BigInt allocator"
    );
}

#[test]
fn test_malloc_kind_telemetry_sweep_by_kind() {
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_malloc_kind_telemetry_for_tests();
    let kinds = [
        GC_TYPE_STRING,
        GC_TYPE_CLOSURE,
        GC_TYPE_PROMISE,
        GC_TYPE_BIGINT,
        GC_TYPE_ERROR,
    ];
    let baselines: Vec<(u64, u64)> = kinds
        .iter()
        .map(|&kind| {
            let stats = malloc_kind_telemetry_for_tests(kind);
            (stats.survivor_count, stats.survivor_bytes)
        })
        .collect();

    let mut dead = Vec::new();
    let mut live = Vec::new();
    for &kind in &kinds {
        let dead_ptr = alloc_malloc_kind_test_object(kind);
        let live_ptr = alloc_malloc_kind_test_object(kind);
        unsafe {
            dead.push((kind, header_from_user_ptr(dead_ptr) as usize));
            live.push((kind, header_from_user_ptr(live_ptr) as usize));
        }
    }

    let dead_headers: Vec<usize> = dead.iter().map(|&(_, header)| header).collect();
    mark_existing_malloc_and_arena_objects_except(&dead_headers);
    let dead_bytes: Vec<u64> = dead
        .iter()
        .map(|&(_, header)| unsafe { (*(header as *mut GcHeader)).size as u64 })
        .collect();
    let live_bytes: Vec<u64> = live
        .iter()
        .map(|&(_, header)| unsafe { (*(header as *mut GcHeader)).size as u64 })
        .collect();

    let freed = sweep_malloc_objects();
    assert_eq!(
        freed,
        dead_bytes.iter().sum::<u64>(),
        "target sweep should reclaim only the intentionally-dead malloc objects"
    );

    for &(_, header) in &dead {
        assert!(
            !MALLOC_STATE.with(|s| s
                .borrow()
                .objects
                .iter()
                .any(|&tracked| tracked as usize == header)),
            "dead malloc header should be removed from tracking"
        );
    }
    for &(_, header) in &live {
        assert!(
            MALLOC_STATE.with(|s| s
                .borrow()
                .objects
                .iter()
                .any(|&tracked| tracked as usize == header)),
            "live malloc header should remain tracked"
        );
    }

    for (idx, &kind) in kinds.iter().enumerate() {
        let stats = malloc_kind_telemetry_for_tests(kind);
        assert_eq!(stats.allocated_count, 2, "{}", gc_type_name(kind));
        assert_eq!(
            stats.allocated_bytes,
            dead_bytes[idx] + live_bytes[idx],
            "{}",
            gc_type_name(kind)
        );
        assert_eq!(stats.freed_count, 1, "{}", gc_type_name(kind));
        assert_eq!(stats.freed_bytes, dead_bytes[idx], "{}", gc_type_name(kind));
        assert_eq!(
            stats.survivor_count,
            baselines[idx].0 + 1,
            "{}",
            gc_type_name(kind)
        );
        assert_eq!(
            stats.survivor_bytes,
            baselines[idx].1 + live_bytes[idx],
            "{}",
            gc_type_name(kind)
        );
    }
    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_malloc_kind_telemetry_batch_and_realloc() {
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_malloc_kind_telemetry_for_tests();
    let baseline = malloc_kind_telemetry_for_tests(GC_TYPE_STRING);
    let sizes = [8usize, 16, 24];
    let ptrs = gc_malloc_batch(&sizes, GC_TYPE_STRING);
    let old_total = unsafe { (*header_from_user_ptr(ptrs[1])).size as u64 };
    let new_ptr = gc_realloc(ptrs[1], 64);
    let new_total = unsafe { (*header_from_user_ptr(new_ptr)).size as u64 };
    let allocated_bytes = sizes
        .iter()
        .map(|size| (GC_HEADER_SIZE + size) as u64)
        .sum::<u64>();

    let stats = malloc_kind_telemetry_for_tests(GC_TYPE_STRING);
    assert_eq!(stats.allocated_count, sizes.len() as u64);
    assert_eq!(stats.allocated_bytes, allocated_bytes);
    assert_eq!(stats.realloc_count, 1);
    assert_eq!(stats.realloc_old_bytes, old_total);
    assert_eq!(stats.realloc_new_bytes, new_total);
    assert_eq!(
        stats.survivor_count,
        baseline.survivor_count + sizes.len() as u64
    );
    assert_eq!(
        stats.survivor_bytes,
        baseline
            .survivor_bytes
            .saturating_add(allocated_bytes)
            .saturating_sub(old_total)
            .saturating_add(new_total)
    );
    assert!(malloc_user_ptr_tracked(new_ptr));
}

#[test]
fn test_malloc_kind_telemetry_copied_minor_validation_by_kind() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live_child = young_leaf();
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>() + std::mem::size_of::<u64>(),
        GC_TYPE_CLOSURE,
    );
    let capture_slot =
        unsafe { init_test_closure_with_one_capture(live_malloc, ptr_bits(live_child)) };
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));
    let rejected_malloc_probe = (live_malloc as usize).saturating_add(16);
    js_shadow_slot_set(1, ptr_bits(rejected_malloc_probe));
    activate_malloc_registry_for_tests();

    let churn_headers = allocate_dead_malloc_churn_headers(128);
    reset_malloc_kind_telemetry_for_tests();
    trigger_guard.make_malloc_sweep_due();
    let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::ArenaBytes,
        steps_before: Some(GcStepSnapshot::current()),
    });
    let trace = outcome.trace.expect("test requested GC trace capture");

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, true);
    assert!(
        trace.copying_nursery.malloc_validation_lookups > 0,
        "copied-minor should preserve the existing total malloc validation counter"
    );
    let closure_stats = malloc_kind_telemetry_for_tests(GC_TYPE_CLOSURE);
    assert!(
        closure_stats.copied_minor_validation_lookups > 0,
        "live malloc closure validation should be attributed to closure"
    );
    assert!(
        closure_stats.copied_minor_validation_lookups < churn_headers.len() as u64,
        "per-kind validation must scale with reachable malloc candidates, not dead churn"
    );
    let unknown_stats = malloc_kind_telemetry_for_tests(0);
    assert!(
        unknown_stats.copied_minor_validation_lookups > 0,
        "rejected copied-minor malloc validation probes should land in unknown"
    );
    assert_eq!(tracked_malloc_headers_matching(&churn_headers), 0);
    assert!(malloc_user_ptr_tracked(live_malloc));
    let capture_after = unsafe { (*capture_slot & POINTER_MASK) as usize };
    assert_ne!(capture_after, live_child);
    assert!(crate::arena::pointer_in_nursery(capture_after));
}

#[test]
fn test_malloc_kind_telemetry_trace_json() {
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_malloc_kind_telemetry_for_tests();
    let _ptr = gc_malloc(24, GC_TYPE_STRING);
    let trace = GcCycleTrace::new(
        GcCollectionKind::Minor,
        GcTriggerSnapshot {
            kind: GcTriggerKind::Direct,
            steps_before: Some(GcStepSnapshot::current()),
        },
    )
    .expect("test requested GC trace capture");

    let event = trace.into_json(GcStepSnapshot::current());
    let rows = event["malloc_kinds"]
        .as_array()
        .expect("malloc_kinds should be an array");
    assert_eq!(rows.len(), MALLOC_KIND_BUCKET_COUNT);
    for info in gc_type_infos() {
        let kind = info.type_id;
        let row = rows
            .iter()
            .find(|row| row["obj_type"].as_u64() == Some(kind as u64))
            .unwrap_or_else(|| panic!("missing malloc_kinds row for {}", gc_type_name(kind)));
        assert_eq!(row["kind"].as_str(), Some(gc_type_name(kind)));
        for field in [
            "allocated_count",
            "allocated_bytes",
            "realloc_count",
            "realloc_old_bytes",
            "realloc_new_bytes",
            "freed_count",
            "freed_bytes",
            "survivor_count",
            "survivor_bytes",
            "copied_minor_validation_lookups",
        ] {
            assert!(
                row.get(field).and_then(|value| value.as_u64()).is_some(),
                "missing numeric field {field} for {}",
                gc_type_name(kind)
            );
        }
    }
    let string_row = rows
        .iter()
        .find(|row| row["obj_type"].as_u64() == Some(GC_TYPE_STRING as u64))
        .expect("string row should be present");
    assert_eq!(string_row["allocated_count"].as_u64(), Some(1));
    assert_eq!(
        string_row["allocated_bytes"].as_u64(),
        Some((GC_HEADER_SIZE + 24) as u64)
    );
    let unknown_row = rows
        .iter()
        .find(|row| row["obj_type"].as_u64() == Some(0))
        .expect("unknown row should be present");
    assert_eq!(unknown_row["kind"].as_str(), Some("unknown"));
}

/// `MALLOC_STATE` starts small (4096-entry pre-size) and jumps to the
/// heavy 256 k pre-size exactly once, the first time a thread's tracked
/// object count crosses `MALLOC_STATE_HEAVY_LEN_THRESHOLD` — spawn
/// workers keep the small floor, promise-heavy threads keep the
/// rehash-amortization the old flat pre-size existed for.
#[test]
fn malloc_state_capacity_grows_once_for_heavy_threads() {
    std::thread::spawn(|| {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

        // Fresh thread → pristine MALLOC_STATE at the small pre-size.
        let (initial_objects_cap, initial_set_cap) = MALLOC_STATE.with(|s| {
            let s = s.borrow();
            (s.objects.capacity(), s.set.capacity())
        });
        assert!(
            initial_objects_cap < MALLOC_STATE_HEAVY_CAPACITY,
            "objects Vec must not start at the heavy pre-size (got {initial_objects_cap})"
        );
        assert!(
            initial_set_cap < MALLOC_STATE_HEAVY_CAPACITY,
            "pointer set must not start at the heavy pre-size (got {initial_set_cap})"
        );

        // Cross the heavy threshold; the one-shot reserve must trip.
        while MALLOC_STATE.with(|s| s.borrow().objects.len()) < MALLOC_STATE_HEAVY_LEN_THRESHOLD {
            let _ = gc_malloc(16, GC_TYPE_STRING);
        }
        MALLOC_STATE.with(|s| {
            let s = s.borrow();
            assert!(
                s.heavy_capacity_reserved,
                "heavy latch must trip at the threshold"
            );
            assert!(
                s.objects.capacity() >= MALLOC_STATE_HEAVY_CAPACITY,
                "objects Vec must reserve the heavy capacity (got {})",
                s.objects.capacity()
            );
        });
    })
    .join()
    .expect("malloc capacity growth test panicked");
}
