//! Precise encoding does not authorize one heap to mark another heap's roots.
use super::super::*;
use super::support::*;

fn flags(addr: usize) -> u8 {
    unsafe { (*header_from_user_ptr(addr as *const u8)).gc_flags }
}

unsafe fn malloc_string(bytes: &[u8]) -> usize {
    let size = std::mem::size_of::<crate::StringHeader>();
    let user = gc_malloc(size + bytes.len(), GC_TYPE_STRING);
    // GC_STORE_AUDIT(INIT): fresh pointer-free string metadata and bytes.
    (user as *mut crate::StringHeader).write(crate::StringHeader {
        utf16_len: bytes.len() as u32,
        byte_len: bytes.len() as u32,
        capacity: bytes.len() as u32,
        refcount: 0,
        flags: 0,
    });
    // GC_STORE_AUDIT(INIT): fresh string's pointer-free character payload.
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), user.add(size), bytes.len());
    user as usize
}

#[test]
fn foreign_precise_visitors_do_not_mark_or_pin_parent_callbacks() {
    let _guard = GcTestIsolationGuard::new();
    let _roots = ShadowAndGlobalRootResetGuard;
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let payload = b"parent callback keeps its own captured payload";
    let child = unsafe { malloc_string(payload) };
    let size = std::mem::size_of::<crate::closure::ClosureHeader>() + 8;
    let arena_callback = crate::arena::arena_alloc_gc(size, 8, GC_TYPE_CLOSURE) as usize;
    let malloc_callback = gc_malloc(size, GC_TYPE_CLOSURE) as usize;
    for callback in [arena_callback, malloc_callback] {
        unsafe {
            init_test_closure_with_one_capture(callback as *mut u8, string_bits(child));
        }
    }
    let mut arena_root = ptr_bits(arena_callback);
    let mut malloc_root = ptr_bits(malloc_callback);
    js_gc_register_global_root(&mut arena_root as *mut u64 as i64);
    js_gc_register_global_root(&mut malloc_root as *mut u64 as i64);
    let before = [flags(arena_callback), flags(malloc_callback), flags(child)];
    assert!(before.iter().all(|flags| flags & GC_FLAG_MARKED == 0));
    let parent_agent = crate::agent::current_agent();

    let outcomes = std::thread::spawn(move || {
        let worker_agent = crate::agent::enter_worker_agent();
        assert_ne!(worker_agent, parent_agent);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let valid = build_valid_pointer_set();
        let mut outcomes = Vec::new();
        for callback in [arena_callback, malloc_callback] {
            assert!(!valid.contains(&callback));
            let direct_marked = mark_precise_root(PreciseRoot::GcPointer(callback), &valid);
            let mut visitor = RuntimeRootVisitor::for_mark(&valid);
            let mut boxed = ptr_bits(callback);
            let mut raw = callback;
            visitor.visit_nanbox_u64_slot(&mut boxed);
            visitor.visit_usize_slot(&mut raw);
            let pinned_bytes = mark_copy_only_scanner_bits(boxed, &valid, true);
            let pinned =
                CONS_PINNED.with(|pinned| pinned.borrow().contains(&(callback - GC_HEADER_SIZE)));
            outcomes.push((direct_marked, boxed, raw, pinned_bytes, pinned));
        }
        clear_mark_seeds();
        crate::agent::retire_agent(worker_agent);
        outcomes
    })
    .join()
    .unwrap();

    for (callback, outcome) in [arena_callback, malloc_callback].into_iter().zip(outcomes) {
        assert_eq!(outcome, (false, ptr_bits(callback), callback, None, false));
    }
    assert_eq!(
        [flags(arena_callback), flags(malloc_callback), flags(child)],
        before,
        "foreign root visitors changed a parent-owned GC header"
    );

    // Exercise a real owner-side collection, with dead malloc residents proving
    // it swept. The registered callbacks must retain their captured string.
    let dead = allocate_dead_malloc_churn_headers(4);
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert_eq!(tracked_malloc_headers_matching(&dead), 0);
    assert!(malloc_user_ptr_tracked(child as *mut u8));
    assert!(malloc_user_ptr_tracked(malloc_callback as *mut u8));
    unsafe {
        assert_string_bytes(child as *const crate::StringHeader, payload);
    }
}

#[test]
fn local_births_after_census_are_still_precise_roots() {
    let _guard = GcTestIsolationGuard::new();
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let snapshot = build_valid_pointer_set();
    let arena = young_leaf();
    let malloc = unsafe { malloc_string(b"born after the census") };
    for addr in [arena, malloc] {
        assert!(
            !snapshot.contains(&addr),
            "birth must be outside the snapshot"
        );
        let mut visitor = RuntimeRootVisitor::for_mark(&snapshot);
        let mut boxed = string_bits(addr);
        visitor.visit_nanbox_u64_slot(&mut boxed);
        assert_marked_user_ptr(addr, "local post-census birth");
    }
    clear_marks();
    clear_mark_seeds();
}

#[test]
fn local_old_generation_roots_need_no_census_entry() {
    let _guard = GcTestIsolationGuard::new();
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let old = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        8,
        GC_TYPE_CLOSURE,
    ) as usize;
    unsafe {
        init_test_closure(old as *mut u8);
    }
    assert!(crate::arena::pointer_in_old_gen(old));
    // The incoming census deliberately omits this existing old generation.
    // Its live allocator ownership must admit the pointer independently.
    let omitted_old = ValidPointerSet::new();
    assert!(!omitted_old.contains(&old));
    let mut visitor = RuntimeRootVisitor::for_mark(&omitted_old);
    let mut raw = old;
    visitor.visit_usize_slot(&mut raw);
    assert_marked_user_ptr(old, "local old-generation pointer");
    clear_marks();
    clear_mark_seeds();
}

#[test]
fn malloc_root_admission_keeps_the_exact_registry_inactive() {
    std::thread::spawn(|| {
        crate::agent::enter_worker_agent();
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let snapshot = build_valid_pointer_set();
        let addr = unsafe { malloc_string(b"local malloc birth") };
        assert!(!snapshot.contains(&addr));
        let registry = || {
            MALLOC_STATE.with(|state| {
                let state = state.borrow();
                (state.malloc_registry_available(), state.set.capacity())
            })
        };
        assert_eq!(registry(), (false, 0));
        let mut word = string_bits(addr);
        RuntimeRootVisitor::for_mark(&snapshot).visit_nanbox_u64_slot(&mut word);
        assert_marked_user_ptr(addr, "inactive-registry local birth");
        assert_eq!(registry(), (false, 0));
        let rejected = std::thread::spawn(move || {
            crate::agent::enter_worker_agent();
            let omitted = ValidPointerSet::new();
            let marked = mark_precise_root(PreciseRoot::GcPointer(addr), &omitted);
            let state = MALLOC_STATE.with(|state| {
                let state = state.borrow();
                (state.malloc_registry_available(), state.set.capacity())
            });
            (marked, state)
        })
        .join()
        .unwrap();
        assert_eq!(rejected, (false, (false, 0)));
        assert_eq!(registry(), (false, 0));
        clear_marks();
        clear_mark_seeds();
    })
    .join()
    .unwrap();
}
