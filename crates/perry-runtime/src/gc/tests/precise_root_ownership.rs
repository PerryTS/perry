//! Precise encoding does not authorize one heap to mark another heap's roots.
//! Admission is O(1): the arena region descriptor, then the header. A root in
//! another thread's live heap region is dropped silently; every other root that is
//! not this heap's object fails loudly under verification.
use super::super::*;
use super::support::*;

/// Run `f`, which must reject a precise root loudly; return the message.
fn loud_rejection(f: impl FnOnce()) -> String {
    let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .expect_err("a root that is not this heap's object must fail loudly");
    err.downcast_ref::<String>()
        .cloned()
        .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

/// Every precise-root entry point: direct marking, both runtime visitor
/// encodings, and the copy-only pinning scanner.
fn reject_through_every_entry(addr: usize, why: &str) {
    let valid = ValidPointerSet::new();
    let entries: [(&str, Box<dyn Fn()>); 4] = [
        (
            "direct",
            Box::new(|| {
                mark_precise_root(PreciseRoot::GcPointer(addr), &valid);
            }),
        ),
        (
            "nanbox visitor",
            Box::new(|| {
                let mut boxed = ptr_bits(addr);
                RuntimeRootVisitor::for_mark(&valid).visit_nanbox_u64_slot(&mut boxed);
            }),
        ),
        (
            "raw visitor",
            Box::new(|| {
                let mut raw = addr;
                RuntimeRootVisitor::for_mark(&valid).visit_usize_slot(&mut raw);
            }),
        ),
        (
            "copy-only pin",
            Box::new(|| {
                mark_copy_only_scanner_bits(ptr_bits(addr), &valid, true);
            }),
        ),
    ];
    for (entry, run) in entries {
        let message = loud_rejection(run);
        assert!(
            message.contains("invalid precise root header") && message.contains(why),
            "{entry}: expected a loud `{why}` rejection, got {message:?}"
        );
    }
}

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
        // The parent's arena region descriptor proves another live heap owns
        // this callback: dropped silently, its header never read or written.
        assert_eq!(
            crate::arena::classify_region_ownership(arena_callback),
            crate::arena::RegionOwnership::OtherHeap
        );
        // No O(1) descriptor names a malloc allocation's agent. Verification
        // proves it is not this heap's and fails loudly, before any write.
        reject_through_every_entry(malloc_callback, "not this heap's allocation");
        let mut outcomes = Vec::new();
        for callback in [arena_callback] {
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

    assert_eq!(outcomes.len(), 1);
    for (callback, outcome) in [arena_callback].into_iter().zip(outcomes) {
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
fn release_admission_reads_no_malloc_state() {
    std::thread::spawn(|| {
        crate::agent::enter_worker_agent();
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let snapshot = build_valid_pointer_set();
        // Many live malloc residents: a search over them would be the cost.
        let residents: Vec<usize> = (0..4096)
            .map(|i| unsafe { malloc_string(format!("resident {i}").as_bytes()) })
            .collect();
        let addr = unsafe { malloc_string(b"local malloc birth") };
        assert!(!snapshot.contains(&addr));
        let registry = || {
            MALLOC_STATE.with(|state| {
                let state = state.borrow();
                (state.malloc_registry_available(), state.set.capacity())
            })
        };
        assert_eq!(registry(), (false, 0));
        {
            let _release = PreciseRootVerificationOff::new();
            // Holding the malloc state exclusively proves admission never
            // consults it: any lookup would panic with a borrow error.
            MALLOC_STATE.with(|state| {
                let _exclusive = state.borrow_mut();
                let mut word = string_bits(addr);
                RuntimeRootVisitor::for_mark(&snapshot).visit_nanbox_u64_slot(&mut word);
                let mut raw = residents[17];
                RuntimeRootVisitor::for_mark(&snapshot).visit_usize_slot(&mut raw);
            });
        }
        assert_marked_user_ptr(addr, "release admission of a local malloc birth");
        assert_marked_user_ptr(residents[17], "release admission of a malloc resident");
        // Verification checks membership without activating the registry.
        let other = residents[4000];
        RuntimeRootVisitor::for_mark(&snapshot).visit_usize_slot(&mut { other });
        assert_marked_user_ptr(other, "verified admission of a malloc resident");
        assert_eq!(registry(), (false, 0));
        clear_marks();
        clear_mark_seeds();
    })
    .join()
    .unwrap();
}

/// Producer bugs a precise root can carry: a word that names no object, an
/// arena header outside this heap's regions, and a well-formed header that is
/// not this heap's allocation (a stale or foreign malloc word). Each fails
/// loudly through every entry point, and no header is written.
#[test]
fn mis_typed_local_roots_fail_loudly() {
    let _guard = GcTestIsolationGuard::new();
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let header_word = |obj_type: u8, gc_flags: u8, size: u32| -> u64 {
        let header = GcHeader {
            obj_type,
            gc_flags,
            _reserved: 0,
            size,
        };
        unsafe { std::mem::transmute::<GcHeader, u64>(header) }
    };
    let cases = [
        (0u64, "malformed header"),
        (
            header_word(GC_TYPE_STRING, GC_FLAG_ARENA, 32),
            "arena header outside this heap's regions",
        ),
        (
            header_word(GC_TYPE_STRING, 0, 32),
            "not this heap's allocation",
        ),
    ];
    for (word, why) in cases {
        // Private memory that is not a GC allocation: header word + payload.
        let mut cell: Box<[u64; 4]> = Box::new([word, 0, 0, 0]);
        let addr = cell.as_mut_ptr() as usize + GC_HEADER_SIZE;
        assert_eq!(
            crate::arena::classify_region_ownership(addr),
            crate::arena::RegionOwnership::Outside
        );
        reject_through_every_entry(addr, why);
        assert_eq!(cell[0], word, "a rejected root's header was written");
        assert!(!CONS_PINNED.with(|pinned| pinned.borrow().contains(&(addr - GC_HEADER_SIZE))));
    }
    clear_mark_seeds();
}

/// #12271's JSValue-root contract: a bare address naming one of this heap's
/// objects in a JSValue root is a producer bug, reported when the slot is
/// registered and when a collector scans it. An address-shaped number that
/// names no object of this heap stays a number.
#[test]
fn bare_heap_addresses_in_jsvalue_roots_fail_loudly() {
    let _guard = GcTestIsolationGuard::new();
    let _roots = ShadowAndGlobalRootResetGuard;
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arena = young_leaf();
    let malloc = unsafe { malloc_string(b"bare malloc address") };
    let valid = ValidPointerSet::new();
    for addr in [arena, malloc] {
        let mut slot = addr as u64;
        let message = loud_rejection(|| {
            register_global_root(&mut slot);
        });
        assert!(
            message.contains("untagged heap address") && message.contains("global"),
            "registration: {message:?}"
        );
        let message = loud_rejection(|| {
            mark_precise_root(PreciseRoot::JSValue(addr as u64), &valid);
        });
        assert!(
            message.contains("untagged heap address") && message.contains("scanned"),
            "scan: {message:?}"
        );
        let message = loud_rejection(|| {
            mark_copy_only_scanner_bits(addr as u64, &valid, true);
        });
        assert!(message.contains("copy-only"), "copy-only scan: {message:?}");
    }
    // A number whose bits look like an address but name no object of this heap.
    let cell: Box<[u64; 2]> = Box::new([0, 0]);
    let number = cell.as_ptr() as u64 + 8;
    let mut slot = number;
    js_gc_register_global_root(&mut slot as *mut u64 as i64);
    assert!(!mark_precise_root(PreciseRoot::JSValue(number), &valid));
    clear_mark_seeds();
}
