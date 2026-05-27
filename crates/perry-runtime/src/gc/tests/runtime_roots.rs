use super::super::*;
use super::support::*;
use std::cell::Cell;
mod callback_scanners;

fn assert_panics_with(expected: &str, f: impl FnOnce()) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    let Err(payload) = result else {
        panic!("expected panic containing {expected:?}");
    };
    let message = if let Some(s) = payload.downcast_ref::<&str>() {
        *s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.as_str()
    } else {
        "<non-string panic>"
    };
    assert!(
        message.contains(expected),
        "panic message {message:?} did not contain {expected:?}"
    );
}

fn force_next_general_arena_alloc_slow() {
    const TEST_BLOCK_SIZE: usize = 1024 * 1024;
    let _ = crate::arena::arena_alloc(TEST_BLOCK_SIZE, 8);
}

fn test_empty_copy_only_root_scanner(_mark: &mut dyn FnMut(f64)) {}

fn assert_moved_callable_closure(bits: u64, original: usize) {
    let rewritten = assert_moved_closure_ptr(bits, original);
    assert_eq!(
        crate::closure::js_closure_call0(rewritten as *const crate::closure::ClosureHeader),
        0.0
    );
}

fn assert_moved_closure_ptr(bits: u64, original: usize) -> usize {
    assert_eq!(bits & TAG_MASK, POINTER_TAG);
    let rewritten = (bits & POINTER_MASK) as usize;
    assert_ne!(
        rewritten, original,
        "runtime callback root should be rewritten after copied-minor GC"
    );
    assert!(crate::arena::pointer_in_nursery(rewritten));
    rewritten
}

#[test]
fn test_scoped_root_scanner_registry_guard_restores_counts() {
    let before = root_scanner_registry_counts();
    {
        let _guard = ScopedRootScannerRegistryGuard::new();
        gc_register_root_scanner(test_empty_copy_only_root_scanner);
        gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);
        let during = root_scanner_registry_counts();
        assert_eq!(during.0, before.0 + 1);
        assert_eq!(during.1, before.1 + 1);
        assert_eq!(during.2, before.2);
        assert_eq!(during.3, before.3);
    }
    assert_eq!(root_scanner_registry_counts(), before);
}

thread_local! {
    static JSON_TAPE_HOOK_TARGET: std::cell::Cell<Option<crate::json_tape::JsonTapeSafepoint>> =
        const { std::cell::Cell::new(None) };
    static JSON_TAPE_HOOK_FIRED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static JSON_TAPE_HOOK_PTR: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn json_tape_force_minor_gc_hook(point: crate::json_tape::JsonTapeSafepoint, ptr: usize) {
    let should_collect = JSON_TAPE_HOOK_TARGET.with(|target| target.get() == Some(point))
        && JSON_TAPE_HOOK_FIRED.with(|fired| !fired.get());
    if !should_collect {
        return;
    }
    JSON_TAPE_HOOK_FIRED.with(|fired| fired.set(true));
    JSON_TAPE_HOOK_PTR.with(|slot| slot.set(ptr));
    let _ = crate::gc::gc_collect_minor();
}

struct JsonTapeSafepointHookGuard {
    previous: Option<crate::json_tape::JsonTapeSafepointHook>,
}

impl JsonTapeSafepointHookGuard {
    fn new(target: crate::json_tape::JsonTapeSafepoint) -> Self {
        JSON_TAPE_HOOK_TARGET.with(|slot| slot.set(Some(target)));
        JSON_TAPE_HOOK_FIRED.with(|slot| slot.set(false));
        JSON_TAPE_HOOK_PTR.with(|slot| slot.set(0));
        let previous =
            crate::json_tape::test_set_safepoint_hook(Some(json_tape_force_minor_gc_hook));
        Self { previous }
    }

    fn fired_ptr(&self) -> usize {
        assert!(
            JSON_TAPE_HOOK_FIRED.with(|slot| slot.get()),
            "JSON tape safepoint hook did not fire"
        );
        JSON_TAPE_HOOK_PTR.with(|slot| slot.get())
    }
}

impl Drop for JsonTapeSafepointHookGuard {
    fn drop(&mut self) {
        crate::json_tape::test_set_safepoint_hook(self.previous);
        JSON_TAPE_HOOK_TARGET.with(|slot| slot.set(None));
        JSON_TAPE_HOOK_FIRED.with(|slot| slot.set(false));
        JSON_TAPE_HOOK_PTR.with(|slot| slot.set(0));
    }
}

#[test]
fn test_forwarding_pointer_roundtrip() {
    // Allocate a nursery object, simulate evacuation by copying
    // its bytes into an old-gen alloc, install the forwarding
    // address in the nursery header. Read back via
    // forwarding_address to confirm round-trip.
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        // Pre-condition: not forwarded yet.
        let nursery_hdr = header_from_user_ptr(nursery_user);
        assert_eq!((*nursery_hdr).gc_flags & GC_FLAG_FORWARDED, 0);
        // Install forwarding pointer.
        set_forwarding_address(nursery_hdr as *mut GcHeader, old_user);
        // Post-condition: flag set, address readable.
        assert_ne!((*nursery_hdr).gc_flags & GC_FLAG_FORWARDED, 0);
        assert_eq!(forwarding_address(nursery_hdr), old_user);
    }
}

#[test]
fn test_forwarding_does_not_disturb_other_flags() {
    // Setting FORWARDED must preserve every other gc_flags bit.
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        let hdr = header_from_user_ptr(user) as *mut GcHeader;
        // Set a few unrelated flags.
        (*hdr).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED | GC_FLAG_HAS_SURVIVED;
        let before = (*hdr).gc_flags;
        set_forwarding_address(hdr, old);
        let after = (*hdr).gc_flags;
        assert_eq!(after & GC_FLAG_FORWARDED, GC_FLAG_FORWARDED);
        // Every bit that was set before stays set.
        assert_eq!(
            after & before,
            before,
            "forwarding installation cleared an existing flag"
        );
    }
}

#[test]
fn test_forwarding_pointer_value_is_8_bytes_at_user_offset_zero() {
    // The forwarding pointer is stored in the first 8 bytes of
    // the user payload. This invariant is load-bearing for any
    // future walker that wants to skip over forwarded objects
    // by reading the new address inline. Verify by direct
    // pointer arithmetic.
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let target = 0x12345678_9ABCDEF0_u64 as *mut u8;
    unsafe {
        let hdr = header_from_user_ptr(nursery_user) as *mut GcHeader;
        set_forwarding_address(hdr, target);
        // Read directly: user_ptr cast to *const *mut u8.
        let raw = nursery_user as *const *mut u8;
        assert_eq!(*raw, target);
    }
}

#[test]
fn test_rewrite_mutable_root_slots_updates_shadow_and_global_roots() {
    let _guard = ShadowAndGlobalRootResetGuard;
    reset_shadow_stack();
    reset_global_roots();

    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        let nursery_hdr = header_from_user_ptr(nursery_user) as *mut GcHeader;
        set_forwarding_address(nursery_hdr, old_user);
    }

    let shadow_bits = POINTER_TAG | ((nursery_user as u64) & POINTER_MASK);
    let expected_shadow_bits = POINTER_TAG | ((old_user as u64) & POINTER_MASK);
    let shadow = js_shadow_frame_push(1);
    js_shadow_slot_set(0, shadow_bits);

    let mut global_bits = nursery_user as u64;
    js_gc_register_global_root((&mut global_bits as *mut u64) as i64);

    rewrite_mutable_root_slots(&valid_ptrs, None);

    assert_eq!(
        js_shadow_slot_get(0),
        expected_shadow_bits,
        "shadow stack slot should be rewritten to the forwarding target"
    );
    assert_eq!(
        global_bits, old_user as u64,
        "registered global root slot should be rewritten in place"
    );

    js_shadow_frame_pop(shadow);
}

#[test]
fn test_rewrite_mutable_root_slots_follows_forwarding_chain() {
    let _guard = ShadowAndGlobalRootResetGuard;
    reset_shadow_stack();

    let first = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let second = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let final_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(header_from_user_ptr(first) as *mut GcHeader, second);
        set_forwarding_address(header_from_user_ptr(second) as *mut GcHeader, final_user);
    }

    let shadow_bits = POINTER_TAG | (first as u64 & POINTER_MASK);
    let expected_bits = POINTER_TAG | (final_user as u64 & POINTER_MASK);
    let shadow = js_shadow_frame_push(1);
    js_shadow_slot_set(0, shadow_bits);

    rewrite_mutable_root_slots(&valid_ptrs, None);

    assert_eq!(
        js_shadow_slot_get(0),
        expected_bits,
        "shadow stack slot should be rewritten through every forwarding hop"
    );

    js_shadow_frame_pop(shadow);
}

#[test]
fn test_runtime_root_visitor_marks_and_rewrites_nanbox_slot() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let nursery_hdr = unsafe { header_from_user_ptr(nursery_user) as *mut GcHeader };
    unsafe {
        set_forwarding_address(nursery_hdr, old_user);
    }

    let mut slot = f64::from_bits(POINTER_TAG | (nursery_user as u64 & POINTER_MASK));
    RuntimeRootVisitor::for_mark(&valid_ptrs).visit_nanbox_f64_slot(&mut slot);
    unsafe {
        assert_ne!((*nursery_hdr).gc_flags & GC_FLAG_MARKED, 0);
    }

    RuntimeRootVisitor::for_rewrite(&valid_ptrs).visit_nanbox_f64_slot(&mut slot);
    assert_eq!(
        slot.to_bits(),
        POINTER_TAG | (old_user as u64 & POINTER_MASK)
    );
}

#[test]
fn test_implicit_this_root_scanner_marks_and_rewrites() {
    // Regression for #1813. The implicit-`this` cell holds the NaN-boxed
    // receiver across a dynamically-dispatched non-arrow method body. Under a
    // moving GC triggered from inside that body (the @perryts/mysql
    // Pool.acquire → handshake → nativeScramble path under concurrent load)
    // the receiver relocates. The scanner must (a) MARK it so it is not swept
    // when the cell is its only root, and (b) REWRITE the cell to the moved
    // copy so the body's next `this`-derived dispatch derefs live memory
    // instead of the stale slot (the reported SIGSEGV in js_native_call_method).
    clear_marks();
    clear_mark_seeds();
    let prev_this = crate::object::js_implicit_this_get();

    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let nursery_hdr = unsafe { header_from_user_ptr(nursery_user) as *mut GcHeader };

    crate::object::js_implicit_this_set(f64::from_bits(ptr_bits(nursery_user as usize)));

    // Mark phase: the live receiver must be discovered as a root.
    crate::object::scan_implicit_this_roots_mut(&mut RuntimeRootVisitor::for_mark(&valid_ptrs));
    unsafe {
        assert_ne!(
            (*nursery_hdr).gc_flags & GC_FLAG_MARKED,
            0,
            "IMPLICIT_THIS scanner must mark the receiver so GC does not sweep `this`"
        );
    }

    // Rewrite phase: the cell must follow the forwarding pointer.
    unsafe {
        set_forwarding_address(nursery_hdr, old_user);
    }
    crate::object::scan_implicit_this_roots_mut(&mut RuntimeRootVisitor::for_rewrite(&valid_ptrs));
    assert_eq!(
        crate::object::js_implicit_this_get().to_bits(),
        ptr_bits(old_user as usize),
        "IMPLICIT_THIS must be rewritten to the receiver's relocated copy (#1813)"
    );

    // Idle / undefined cell must be a no-op (the default state between calls).
    crate::object::js_implicit_this_set(f64::from_bits(crate::value::TAG_UNDEFINED));
    crate::object::scan_implicit_this_roots_mut(&mut RuntimeRootVisitor::for_rewrite(&valid_ptrs));
    assert_eq!(
        crate::object::js_implicit_this_get().to_bits(),
        crate::value::TAG_UNDEFINED,
        "scanning the idle implicit-`this` cell must leave TAG_UNDEFINED untouched"
    );

    crate::object::js_implicit_this_set(prev_this);
    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_runtime_root_visitor_rewrites_raw_pointer_slots() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(nursery_user) as *mut GcHeader,
            old_user,
        );
    }

    let mut mut_ptr = nursery_user;
    let mut const_ptr = nursery_user as *const u8;
    let mut usize_slot = nursery_user as usize;
    let mut i64_slot = nursery_user as i64;

    let mut visitor = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
    visitor.visit_raw_mut_ptr_slot(&mut mut_ptr);
    visitor.visit_raw_const_ptr_slot(&mut const_ptr);
    visitor.visit_usize_slot(&mut usize_slot);
    visitor.visit_i64_slot(&mut i64_slot);

    assert_eq!(mut_ptr, old_user);
    assert_eq!(const_ptr, old_user as *const u8);
    assert_eq!(usize_slot, old_user as usize);
    assert_eq!(i64_slot, old_user as i64);
}

/// Issue #1790: the class static-inheritance side-tables
/// (`CLASS_PROTOTYPE_OBJECTS`, `CLASS_PARENT_CLOSURES`) store the heap parent
/// as a raw `usize`. `scan_class_inheritance_roots_mut` must (a) MARK each
/// stored parent as a live root so it survives a collection that can't
/// otherwise reach it, and (b) REWRITE the stored address after the parent is
/// evacuated, so the static-inheritance walk (`Sub.ast` / inherited static
/// methods) resolves to the moved object rather than a freed/stale one.
#[test]
fn test_class_inheritance_side_table_roots_mark_and_rewrite() {
    use crate::object::{
        scan_class_inheritance_roots_mut, test_class_parent_closure_root,
        test_class_prototype_object_root, test_clear_class_inheritance_roots,
        test_seed_class_inheritance_roots, test_seed_class_parent_closure_root,
    };

    const PROTO_CID: u32 = 0xDEAD_0001;
    const CLOSURE_CID: u32 = 0xDEAD_0002;

    clear_marks();
    clear_mark_seeds();

    // Allocate the two "parent" objects in the nursery before snapshotting the
    // valid-pointer set so the mark phase recognizes them as roots.
    let proto_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let closure_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let proto_old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let closure_old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let proto_hdr = unsafe { header_from_user_ptr(proto_user) as *mut GcHeader };
    let closure_hdr = unsafe { header_from_user_ptr(closure_user) as *mut GcHeader };

    test_seed_class_inheritance_roots(PROTO_CID, proto_user as usize);
    test_seed_class_parent_closure_root(CLOSURE_CID, closure_user as usize);

    // Mark phase: both parents become live roots.
    scan_class_inheritance_roots_mut(&mut RuntimeRootVisitor::for_mark(&valid_ptrs));
    unsafe {
        assert_ne!(
            (*proto_hdr).gc_flags & GC_FLAG_MARKED,
            0,
            "CLASS_PROTOTYPE_OBJECTS parent must be marked as a root"
        );
        assert_ne!(
            (*closure_hdr).gc_flags & GC_FLAG_MARKED,
            0,
            "CLASS_PARENT_CLOSURES parent must be marked as a root"
        );
    }

    // Simulate evacuation, then run the rewrite phase: the stored raw pointers
    // follow the forwarding address.
    unsafe {
        set_forwarding_address(proto_hdr, proto_old);
        set_forwarding_address(closure_hdr, closure_old);
    }
    scan_class_inheritance_roots_mut(&mut RuntimeRootVisitor::for_rewrite(&valid_ptrs));

    assert_eq!(
        test_class_prototype_object_root(PROTO_CID),
        proto_old as usize,
        "CLASS_PROTOTYPE_OBJECTS parent must be rewritten to the evacuated address"
    );
    assert_eq!(
        test_class_parent_closure_root(CLOSURE_CID),
        closure_old as usize,
        "CLASS_PARENT_CLOSURES parent must be rewritten to the evacuated address"
    );

    // A verify pass must not panic now that the slots point at the live
    // (non-forwarded) evacuated objects.
    scan_class_inheritance_roots_mut(&mut RuntimeRootVisitor::for_verify(
        &valid_ptrs,
        "class inheritance side-table roots (test)",
    ));

    test_clear_class_inheritance_roots(PROTO_CID, CLOSURE_CID);
    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_runtime_root_visitor_rewrites_cell_and_atomic_slots() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(nursery_user) as *mut GcHeader,
            old_user,
        );
    }

    let cell = Cell::new(f64::from_bits(
        POINTER_TAG | (nursery_user as u64 & POINTER_MASK),
    ));
    let atomic = std::sync::atomic::AtomicPtr::new(nursery_user);
    let atomic_i64 = std::sync::atomic::AtomicI64::new(nursery_user as i64);
    let atomic_nanbox_u64 =
        std::sync::atomic::AtomicU64::new(POINTER_TAG | (nursery_user as u64 & POINTER_MASK));

    let mut visitor = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
    visitor.visit_cell_f64_slot(&cell);
    visitor.visit_atomic_nanbox_u64_slot(
        &atomic_nanbox_u64,
        std::sync::atomic::Ordering::Acquire,
        std::sync::atomic::Ordering::Release,
    );
    visitor.visit_atomic_raw_mut_ptr_slot(
        &atomic,
        std::sync::atomic::Ordering::Acquire,
        std::sync::atomic::Ordering::Release,
    );
    visitor.visit_atomic_i64_slot(
        &atomic_i64,
        std::sync::atomic::Ordering::Acquire,
        std::sync::atomic::Ordering::Release,
    );

    assert_eq!(
        cell.get().to_bits(),
        POINTER_TAG | (old_user as u64 & POINTER_MASK)
    );
    assert_eq!(
        atomic_nanbox_u64.load(std::sync::atomic::Ordering::Acquire),
        POINTER_TAG | (old_user as u64 & POINTER_MASK)
    );
    assert_eq!(atomic.load(std::sync::atomic::Ordering::Acquire), old_user);
    assert_eq!(
        atomic_i64.load(std::sync::atomic::Ordering::Acquire),
        old_user as i64
    );
}

#[test]
fn test_runtime_root_visitor_rewrites_metadata_without_marking() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let nursery_hdr = unsafe { header_from_user_ptr(nursery_user) as *mut GcHeader };
    unsafe {
        set_forwarding_address(nursery_hdr, old_user);
    }

    let mut metadata = nursery_user as usize;
    RuntimeRootVisitor::for_mark(&valid_ptrs).visit_metadata_usize_slot(&mut metadata);
    unsafe {
        assert_eq!(
            (*nursery_hdr).gc_flags & GC_FLAG_MARKED,
            0,
            "metadata-only slots must not become roots"
        );
    }

    RuntimeRootVisitor::for_rewrite(&valid_ptrs).visit_metadata_usize_slot(&mut metadata);
    assert_eq!(metadata, old_user as usize);
}

#[test]
fn test_transient_runtime_handle_slots_mark_and_rewrite() {
    clear_marks();
    clear_mark_seeds();

    let nanbox_f64_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let nanbox_u64_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let raw_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let raw_string_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_STRING);
    let heap_word_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();

    let old_nanbox_f64 = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let old_nanbox_u64 = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let old_raw = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let old_raw_string = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_STRING);
    let old_heap_word = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(nanbox_f64_user) as *mut GcHeader,
            old_nanbox_f64,
        );
        set_forwarding_address(
            header_from_user_ptr(nanbox_u64_user) as *mut GcHeader,
            old_nanbox_u64,
        );
        set_forwarding_address(header_from_user_ptr(raw_user) as *mut GcHeader, old_raw);
        set_forwarding_address(
            header_from_user_ptr(raw_string_user) as *mut GcHeader,
            old_raw_string,
        );
        set_forwarding_address(
            header_from_user_ptr(heap_word_user) as *mut GcHeader,
            old_heap_word,
        );
    }

    let scope = RuntimeHandleScope::new();
    let nanbox_f64 = scope.root_nanbox_f64(f64::from_bits(ptr_bits(nanbox_f64_user as usize)));
    let nanbox_u64 = scope.root_nanbox_u64(string_bits(nanbox_u64_user as usize));
    let raw = scope.root_raw_mut_ptr(raw_user);
    let raw_string = scope.root_string_ptr(raw_string_user as *const crate::StringHeader);
    let heap_word = scope.root_heap_word_u64(heap_word_user as u64);

    let mut marker = RuntimeRootVisitor::for_mark(&valid_ptrs);
    scan_runtime_handle_roots_mut(&mut marker);
    unsafe {
        assert_ne!(
            (*header_from_user_ptr(nanbox_f64_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert_ne!(
            (*header_from_user_ptr(nanbox_u64_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert_ne!(
            (*header_from_user_ptr(raw_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert_ne!(
            (*header_from_user_ptr(raw_string_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert_ne!(
            (*header_from_user_ptr(heap_word_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
    }

    let mut rewriter = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
    scan_runtime_handle_roots_mut(&mut rewriter);

    assert_eq!(
        nanbox_f64.get_nanbox_f64().to_bits(),
        ptr_bits(old_nanbox_f64 as usize)
    );
    assert_eq!(
        nanbox_u64.get_nanbox_u64(),
        string_bits(old_nanbox_u64 as usize)
    );
    assert_eq!(raw.get_raw_mut_ptr::<u8>(), old_raw);
    assert_eq!(
        raw_string.get_raw_const_ptr::<crate::StringHeader>() as *mut u8,
        old_raw_string
    );
    assert_eq!(heap_word.get_heap_word_u64(), old_heap_word as u64);
}

#[test]
fn test_transient_runtime_handle_scope_drop_removes_roots() {
    clear_marks();
    clear_mark_seeds();

    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
    let valid_ptrs = build_valid_pointer_set();

    {
        let scope = RuntimeHandleScope::new();
        let _handle = scope.root_nanbox_u64(ptr_bits(user as usize));
        assert!(RuntimeHandleScope::active_len_for_tests() > 0);
    }
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), 0);

    let mut marker = RuntimeRootVisitor::for_mark(&valid_ptrs);
    scan_runtime_handle_roots_mut(&mut marker);
    unsafe {
        assert_eq!(
            (*header).gc_flags & GC_FLAG_MARKED,
            0,
            "dropped handle scopes must not retain transient roots"
        );
    }
}

#[test]
fn test_set_gc_field_rewrite_reindexes_elements() {
    clear_marks();
    clear_mark_seeds();
    crate::set::test_clear_set_roots();

    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let nursery_bits = ptr_bits(nursery_user as usize);
    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, f64::from_bits(nursery_bits));

    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let old_bits = ptr_bits(old_user as usize);
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(nursery_user) as *mut GcHeader,
            old_user,
        );
    }

    let valid_ptrs = build_valid_pointer_set();
    unsafe {
        rewrite_heap_object_fields(header_from_user_ptr(set as *const u8), &valid_ptrs);
    }

    assert_eq!(crate::set::js_set_value_at(set, 0).to_bits(), old_bits);
    assert_eq!(crate::set::js_set_has(set, f64::from_bits(old_bits)), 1);
    assert_eq!(
        crate::set::js_set_has(set, f64::from_bits(nursery_bits)),
        0,
        "set lookup index should be rebuilt after element rewrites"
    );

    clear_marks();
    clear_mark_seeds();
    crate::set::test_clear_set_roots();
}

#[test]
fn test_transient_runtime_handle_string_concat_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let left_bytes = vec![b'a'; 600_000];
    let right_bytes = vec![b'b'; 600_000];
    let left = crate::string::js_string_from_bytes(left_bytes.as_ptr(), left_bytes.len() as u32);
    let right = crate::string::js_string_from_bytes(right_bytes.as_ptr(), right_bytes.len() as u32);

    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = crate::string::js_string_concat(left, right);

    assert!(
        gc_collection_count() > before,
        "concat allocation should trigger copied-minor GC"
    );
    unsafe {
        assert_eq!((*result).byte_len, 1_200_000);
        let data = (result as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        assert_eq!(*data, b'a');
        assert_eq!(*data.add(599_999), b'a');
        assert_eq!(*data.add(600_000), b'b');
        assert_eq!(*data.add(1_199_999), b'b');
    }
}

#[test]
fn test_dynamic_string_add_roots_left_string_across_rhs_coercion_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let left = crate::string::js_string_from_bytes(b"dyn-left-".as_ptr(), 9);
    let left_value = f64::from_bits(string_bits(left as usize));
    let survivor_before = {
        let snapshot = crate::arena::arena_telemetry_snapshot();
        snapshot.survivor0.in_use_bytes + snapshot.survivor1.in_use_bytes
    };

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = unsafe {
        crate::value::js_dynamic_string_or_number_add(
            left_value,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        )
    };

    assert!(
        gc_collection_count() > before,
        "rhs ToString allocation should trigger copied-minor GC"
    );

    let survivor_after = {
        let snapshot = crate::arena::arena_telemetry_snapshot();
        snapshot.survivor0.in_use_bytes + snapshot.survivor1.in_use_bytes
    };
    assert!(
        survivor_after > survivor_before,
        "copied-minor should move the left string into survivor space"
    );

    let result_value = crate::value::JSValue::from_bits(result.to_bits());
    assert!(result_value.is_string());
    unsafe {
        assert_string_bytes(result_value.as_string_ptr(), b"dyn-left-undefined");
    }
}

#[test]
fn test_dynamic_bigint_add_roots_left_bigint_across_rhs_coercion_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let cases: [(&str, unsafe extern "C" fn(f64, f64) -> f64); 2] = [
        ("dynamic add", crate::value::js_dynamic_add),
        (
            "dynamic string-or-number add",
            crate::value::js_dynamic_string_or_number_add,
        ),
    ];

    for (name, op) in cases {
        let left = crate::bigint::js_bigint_from_i64(41);
        let left_value = crate::value::js_nanbox_bigint(left as i64);
        assert!(crate::arena::pointer_in_nursery(left as usize));
        let left_scope = RuntimeHandleScope::new();
        let left_root = left_scope.root_bigint_ptr(left as *const crate::bigint::BigIntHeader);

        force_next_general_arena_alloc_slow();
        trigger_guard.make_arena_trigger_due();
        let before = gc_collection_count();
        let result = unsafe { op(left_value, 1.0) };
        assert!(
            gc_collection_count() > before,
            "{name} rhs BigInt coercion should trigger copied-minor GC"
        );

        let moved_left = left_root.get_raw_const_ptr::<crate::bigint::BigIntHeader>();
        assert_ne!(
            moved_left as usize, left as usize,
            "{name} should rewrite a mutable BigInt root to the moved copy"
        );
        assert_eq!(crate::bigint::js_bigint_to_f64(moved_left), 41.0);

        let result_value = crate::value::JSValue::from_bits(result.to_bits());
        assert!(result_value.is_bigint(), "{name} should return a BigInt");
        assert_eq!(
            crate::bigint::js_bigint_to_f64(result_value.as_bigint_ptr()),
            42.0
        );
    }
}

#[test]
fn test_bigint_method_add_roots_receiver_across_rhs_number_coercion_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let receiver = crate::bigint::js_bigint_from_i64(41);
    let receiver_value = crate::value::js_nanbox_bigint(receiver as i64);
    assert!(crate::arena::pointer_in_nursery(receiver as usize));
    let receiver_scope = RuntimeHandleScope::new();
    let receiver_root =
        receiver_scope.root_bigint_ptr(receiver as *const crate::bigint::BigIntHeader);
    let args = [1.0_f64];

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = unsafe {
        crate::object::js_native_call_method(
            receiver_value,
            b"add".as_ptr() as *const i8,
            3,
            args.as_ptr(),
            args.len(),
        )
    };

    assert!(
        gc_collection_count() > before,
        "BigInt method RHS number coercion should trigger copied-minor GC"
    );
    let moved_receiver = receiver_root.get_raw_const_ptr::<crate::bigint::BigIntHeader>();
    assert_ne!(
        moved_receiver as usize, receiver as usize,
        "BigInt method receiver root should be rewritten to the moved copy"
    );
    assert_eq!(crate::bigint::js_bigint_to_f64(moved_receiver), 41.0);

    let result_value = crate::value::JSValue::from_bits(result.to_bits());
    assert!(
        result_value.is_bigint(),
        "BigInt add method should return BigInt"
    );
    assert_eq!(
        crate::bigint::js_bigint_to_f64(result_value.as_bigint_ptr()),
        42.0
    );
}

#[test]
fn test_string_method_split_roots_receiver_across_separator_materialization_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    activate_malloc_registry_for_tests();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let receiver = crate::string::js_string_from_bytes(b"a,b,c".as_ptr(), 5);
    let receiver_value = f64::from_bits(string_bits(receiver as usize));
    assert!(crate::arena::pointer_in_nursery(receiver as usize));
    let receiver_scope = RuntimeHandleScope::new();
    let receiver_root = receiver_scope.root_string_ptr(receiver);
    let sep = crate::value::JSValue::try_short_string(b",").unwrap();
    let args = [f64::from_bits(sep.bits())];

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = unsafe {
        crate::object::js_native_call_method(
            receiver_value,
            b"split".as_ptr() as *const i8,
            5,
            args.as_ptr(),
            args.len(),
        )
    };

    assert!(
        gc_collection_count() > before,
        "split separator materialization should trigger copied-minor GC"
    );
    let moved_receiver = receiver_root.get_raw_const_ptr::<crate::StringHeader>();
    assert_ne!(
        moved_receiver as usize, receiver as usize,
        "split receiver root should be rewritten to the moved copy"
    );
    unsafe {
        assert_string_bytes(moved_receiver, b"a,b,c");
        let arr = (result.to_bits() & POINTER_MASK) as *const crate::array::ArrayHeader;
        assert_eq!(crate::array::js_array_length(arr), 3);
        let expected: [&[u8]; 3] = [b"a", b"b", b"c"];
        for (i, expected) in expected.iter().enumerate() {
            let value = crate::array::js_array_get(arr, i as u32);
            assert!(value.is_string(), "split element {i} should be a string");
            assert_string_bytes(value.as_string_ptr(), expected);
        }
    }
}

#[test]
fn test_string_method_replace_roots_receiver_across_pattern_materialization_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    activate_malloc_registry_for_tests();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let receiver = crate::string::js_string_from_bytes(b"a-a".as_ptr(), 3);
    let receiver_value = f64::from_bits(string_bits(receiver as usize));
    assert!(crate::arena::pointer_in_nursery(receiver as usize));
    let receiver_scope = RuntimeHandleScope::new();
    let receiver_root = receiver_scope.root_string_ptr(receiver);
    let pattern = crate::value::JSValue::try_short_string(b"-").unwrap();
    let replacement = crate::value::JSValue::try_short_string(b":").unwrap();
    let args = [
        f64::from_bits(pattern.bits()),
        f64::from_bits(replacement.bits()),
    ];

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = unsafe {
        crate::object::js_native_call_method(
            receiver_value,
            b"replace".as_ptr() as *const i8,
            7,
            args.as_ptr(),
            args.len(),
        )
    };

    assert!(
        gc_collection_count() > before,
        "replace pattern materialization should trigger copied-minor GC"
    );
    let moved_receiver = receiver_root.get_raw_const_ptr::<crate::StringHeader>();
    assert_ne!(
        moved_receiver as usize, receiver as usize,
        "replace receiver root should be rewritten to the moved copy"
    );
    unsafe {
        assert_string_bytes(moved_receiver, b"a-a");
        let result_value = crate::value::JSValue::from_bits(result.to_bits());
        assert!(result_value.is_string(), "replace should return a string");
        assert_string_bytes(result_value.as_string_ptr(), b"a:a");
    }
}

#[test]
fn test_transient_runtime_handle_array_push_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let arr = crate::array::js_array_alloc_with_length(200_000);
    let value = crate::string::js_string_from_bytes(b"array-payload".as_ptr(), 13);
    let value_bits = string_bits(value as usize);

    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let grown = crate::array::js_array_push_f64(arr, f64::from_bits(value_bits));

    assert!(
        gc_collection_count() > before,
        "array grow should trigger copied-minor GC"
    );
    unsafe {
        assert_eq!((*grown).length, 200_001);
        let elements =
            (grown as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const u64;
        let stored = *elements.add(200_000);
        assert_eq!(stored & TAG_MASK, STRING_TAG);
        let stored_ptr = (stored & POINTER_MASK) as *const crate::StringHeader;
        assert_ne!(stored_ptr as usize, value as usize);
        assert_string_bytes(stored_ptr, b"array-payload");
    }
}

#[test]
fn test_transient_runtime_handle_object_set_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let obj = crate::object::js_object_alloc(0, 1);
    js_shadow_slot_set(0, ptr_bits(obj as usize));
    let key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
    let value = crate::string::js_string_from_bytes(b"object-payload".as_ptr(), 14);
    force_next_general_arena_alloc_slow();

    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    crate::object::js_object_set_field_by_name(
        obj,
        key,
        f64::from_bits(string_bits(value as usize)),
    );

    assert!(
        gc_collection_count() > before,
        "keys-array allocation should trigger copied-minor GC"
    );
    let obj_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    unsafe {
        assert!(!(*obj_after).keys_array.is_null());
        let stored_value = crate::object::js_object_get_field(obj_after, 0).bits();
        assert_eq!(stored_value & TAG_MASK, STRING_TAG);
        let stored_value_ptr = (stored_value & POINTER_MASK) as *const crate::StringHeader;
        assert_ne!(stored_value_ptr as usize, value as usize);
        assert_string_bytes(stored_value_ptr, b"object-payload");

        let key_value = crate::array::js_array_get((*obj_after).keys_array, 0).bits();
        assert_eq!(key_value & TAG_MASK, STRING_TAG);
        let stored_key_ptr = (key_value & POINTER_MASK) as *const crate::StringHeader;
        assert_ne!(stored_key_ptr as usize, key as usize);
        assert_string_bytes(stored_key_ptr, b"name");
    }
}

#[test]
fn test_transient_runtime_handle_closure_captures_gc() {
    extern "C" fn captured_func(_closure: *const crate::closure::ClosureHeader) -> f64 {
        0.0
    }

    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);
    crate::closure::test_clear_singleton_closure_caches();

    let captured = crate::string::js_string_from_bytes(b"closure-payload".as_ptr(), 15);
    let captures = [string_bits(captured as usize)];

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let closure = crate::closure::js_closure_alloc_with_captures_singleton(
        captured_func as *const u8,
        1,
        captures.as_ptr(),
    );

    assert!(
        gc_collection_count() > before,
        "closure arena allocation should trigger copied-minor GC"
    );
    unsafe {
        let capture_slot = (closure as *const u8)
            .add(std::mem::size_of::<crate::closure::ClosureHeader>())
            as *const u64;
        let stored = *capture_slot;
        assert_eq!(stored & TAG_MASK, STRING_TAG);
        let stored_ptr = (stored & POINTER_MASK) as *const crate::StringHeader;
        assert_ne!(stored_ptr as usize, captured as usize);
        assert_string_bytes(stored_ptr, b"closure-payload");
    }

    let entries =
        crate::closure::test_captured_singleton_closure_cache_entries(captured_func as *const u8);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0.len(), 1);
    assert_ne!(entries[0].0[0], captures[0]);
    assert_eq!(entries[0].0[0] & TAG_MASK, STRING_TAG);
    crate::closure::test_clear_singleton_closure_caches();
}

extern "C" fn test_reviver_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    _key: f64,
    value: f64,
) -> f64 {
    let _ = crate::gc::gc_collect_minor();
    value
}

thread_local! {
    static TEST_REVIVER_CLOSURE_VISITS: Cell<u32> = const { Cell::new(0) };
}

extern "C" fn test_reviver_count_closure_leaf(
    _closure: *const crate::closure::ClosureHeader,
    _key: f64,
    value: f64,
) -> f64 {
    let bits = value.to_bits();
    if bits & TAG_MASK == POINTER_TAG {
        let ptr = (bits & POINTER_MASK) as *const u8;
        if !ptr.is_null() && (ptr as usize) >= GC_HEADER_SIZE + 0x1000 {
            let header = unsafe { ptr.sub(GC_HEADER_SIZE) as *const GcHeader };
            let is_closure = unsafe { (*header).obj_type == GC_TYPE_CLOSURE };
            if is_closure {
                TEST_REVIVER_CLOSURE_VISITS.with(|visits| visits.set(visits.get() + 1));
            }
        }
    }
    value
}

extern "C" fn test_promise_identity_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    _value: f64,
) -> f64 {
    let _ = crate::gc::gc_collect_minor();
    crate::promise::test_current_microtask_value()
}

extern "C" fn test_promise_finally_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    _value: f64,
) -> f64 {
    let _ = crate::gc::gc_collect_minor();
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn test_array_identity_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    _index: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let _ = crate::gc::gc_collect_minor();
    value_handle.get_nanbox_f64()
}

thread_local! {
    static TEST_FOREACH_FORCE_MINOR_VISITS: Cell<u32> = const { Cell::new(0) };
}

extern "C" fn test_foreach_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    key: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let key_handle = scope.root_nanbox_f64(key);
    let _ = crate::gc::gc_collect_minor();
    TEST_FOREACH_FORCE_MINOR_VISITS.with(|visits| visits.set(visits.get() + 1));
    let _ = value_handle.get_nanbox_f64();
    let _ = key_handle.get_nanbox_f64();
    0.0
}

extern "C" fn test_async_hook_init_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    _async_id: f64,
    _type_name: f64,
    _trigger_async_id: f64,
    _resource: f64,
) -> f64 {
    let _ = crate::gc::gc_collect_minor();
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn test_async_hook_event_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    _async_id: f64,
) -> f64 {
    let _ = crate::gc::gc_collect_minor();
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

thread_local! {
    static TEST_TIMER_ARG_BITS: Cell<u64> = const { Cell::new(0) };
    static TEST_TIMER_CALLED: Cell<bool> = const { Cell::new(false) };
}

extern "C" fn test_timer_capture_arg(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    TEST_TIMER_ARG_BITS.with(|slot| slot.set(arg.to_bits()));
    TEST_TIMER_CALLED.with(|slot| slot.set(true));
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn test_rest_first_value(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let rest_ptr = (rest.to_bits() & POINTER_MASK) as *const crate::array::ArrayHeader;
    if rest_ptr.is_null() || crate::array::js_array_length(rest_ptr) == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    f64::from_bits(crate::array::js_array_get(rest_ptr, 0).bits())
}

static ASYNC_HOOK_RUNTIME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct AsyncHookRuntimeTestGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl AsyncHookRuntimeTestGuard {
    fn new() -> Self {
        let lock = ASYNC_HOOK_RUNTIME_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        crate::async_hooks::reset_for_tests();
        crate::object::test_clear_transition_cache_root();
        crate::object::test_clear_object_cache_roots();
        Self { _lock: lock }
    }
}

impl Drop for AsyncHookRuntimeTestGuard {
    fn drop(&mut self) {
        crate::async_hooks::reset_for_tests();
        crate::object::test_clear_transition_cache_root();
        crate::object::test_clear_object_cache_roots();
        crate::exception::js_clear_exception();
    }
}

fn test_string_value(bytes: &[u8]) -> f64 {
    let ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(string_bits(ptr as usize))
}

fn assert_moved_string_value(value: f64, original: usize, expected: &[u8]) {
    let bits = value.to_bits();
    assert_eq!(bits & TAG_MASK, STRING_TAG);
    let ptr = (bits & POINTER_MASK) as *const crate::StringHeader;
    assert_ne!(
        ptr as usize, original,
        "heap string value should be refreshed after copied-minor GC"
    );
    assert!(crate::arena::pointer_in_nursery(ptr as usize));
    unsafe {
        assert_string_bytes(ptr, expected);
    }
}

fn hook_options(fields: &[(&[u8], *mut crate::closure::ClosureHeader)]) -> f64 {
    let obj = crate::object::js_object_alloc(0, fields.len() as u32);
    for (name, callback) in fields {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(
            obj,
            key,
            f64::from_bits(ptr_bits(*callback as usize)),
        );
    }
    f64::from_bits(ptr_bits(obj as usize))
}

fn enable_async_hook(fields: &[(&[u8], *mut crate::closure::ClosureHeader)]) -> i64 {
    let options = hook_options(fields);
    let handle = crate::async_hooks::js_async_hooks_create_hook(options);
    crate::async_hooks::js_async_hook_enable(handle);
    handle
}

fn test_array_from_values(values: &[f64]) -> *mut crate::array::ArrayHeader {
    let arr = crate::array::js_array_alloc(values.len() as u32);
    unsafe {
        (*arr).length = values.len() as u32;
    }
    for (i, value) in values.iter().enumerate() {
        crate::array::js_array_set_f64(arr, i as u32, *value);
    }
    arr
}

fn test_pair_array(key: f64, value: f64) -> *mut crate::array::ArrayHeader {
    test_array_from_values(&[key, value])
}

fn test_array_from_pair(pair: *mut crate::array::ArrayHeader) -> *mut crate::array::ArrayHeader {
    test_array_from_values(&[f64::from_bits(ptr_bits(pair as usize))])
}

fn drain_promise_microtasks_for_test() {
    for _ in 0..16 {
        if crate::promise::js_promise_run_microtasks() == 0 {
            return;
        }
    }
    panic!("promise microtask drain did not quiesce");
}

#[test]
fn test_async_hook_option_lookup_roots_callbacks_across_copied_minor_gc() {
    let _async_hook_guard = AsyncHookRuntimeTestGuard::new();
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let init = crate::closure::js_closure_alloc(test_no_capture_singleton_func as *const u8, 0);
    let original = init as usize;
    let options = hook_options(&[(b"init", init)]);

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let _handle = crate::async_hooks::js_async_hooks_create_hook(options);
    assert!(
        gc_collection_count() > before,
        "async hook option key lookup should trigger copied-minor GC"
    );

    let (callback, _resource_bits) = crate::async_hooks::test_async_hooks_scanner_snapshot();
    assert_moved_callable_closure(ptr_bits(callback), original);
}

#[test]
fn test_closure_rest_dispatch_roots_args_during_rest_array_alloc_gc() {
    let _async_hook_guard = AsyncHookRuntimeTestGuard::new();
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    crate::closure::js_register_closure_rest(test_rest_first_value as *const u8, 0);
    let closure = crate::closure::js_closure_alloc(test_rest_first_value as *const u8, 0);
    let value = test_string_value(b"rest-dispatch");
    let original = (value.to_bits() & POINTER_MASK) as usize;
    let args = [value];

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = unsafe { crate::closure::js_closure_call_array(closure as i64, args.as_ptr(), 1) };
    assert!(
        gc_collection_count() > before,
        "rest-array creation should trigger copied-minor GC"
    );
    assert_moved_string_value(result, original, b"rest-dispatch");
}

#[test]
fn test_bound_timer_dispatch_roots_args_during_async_hook_init_gc() {
    let _async_hook_guard = AsyncHookRuntimeTestGuard::new();
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);
    gc_register_mutable_root_scanner(crate::async_hooks::scan_async_hooks_roots_mut);
    gc_register_mutable_root_scanner(crate::timer::scan_timer_roots_mut);

    let init_hook =
        crate::closure::js_closure_alloc(test_async_hook_init_force_minor_gc as *const u8, 0);
    enable_async_hook(&[(b"init", init_hook)]);

    let timer_callback = crate::closure::js_closure_alloc(test_timer_capture_arg as *const u8, 0);
    let timer_callback_original = timer_callback as usize;
    let arg = test_string_value(b"bound-timer-arg");
    let arg_original = (arg.to_bits() & POINTER_MASK) as usize;

    let module_name = b"timers";
    let namespace =
        crate::object::js_create_native_module_namespace(module_name.as_ptr(), module_name.len());
    let bound = crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 3);
    crate::closure::js_closure_set_capture_f64(bound, 0, namespace);
    let method = b"setTimeout";
    crate::closure::js_closure_set_capture_ptr(bound, 1, method.as_ptr() as i64);
    crate::closure::js_closure_set_capture_ptr(bound, 2, method.len() as i64);

    let timer_value = crate::closure::js_closure_call3(
        bound,
        f64::from_bits(ptr_bits(timer_callback as usize)),
        0.0,
        arg,
    );

    let timer_id = (timer_value.to_bits() & POINTER_MASK) as i64;
    let (callback, arg_bits) = crate::timer::test_callback_timer_snapshot(timer_id)
        .expect("scheduled callback timer should remain queued");
    assert_moved_closure_ptr(ptr_bits(callback), timer_callback_original);
    assert_moved_string_value(f64::from_bits(arg_bits), arg_original, b"bound-timer-arg");
    crate::timer::clearTimeout(timer_id);
}

#[test]
fn test_timer_tick_roots_callback_args_and_previous_context_across_hooks() {
    const ALS_HANDLE: i64 = -8_501;

    let _async_hook_guard = AsyncHookRuntimeTestGuard::new();
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);
    gc_register_mutable_root_scanner(crate::async_hooks::scan_async_hooks_roots_mut);
    gc_register_mutable_root_scanner(crate::timer::scan_timer_roots_mut);

    let before_hook =
        crate::closure::js_closure_alloc(test_async_hook_event_force_minor_gc as *const u8, 0);
    let after_hook =
        crate::closure::js_closure_alloc(test_async_hook_event_force_minor_gc as *const u8, 0);
    enable_async_hook(&[(b"before", before_hook), (b"after", after_hook)]);

    crate::async_context::clear_store(ALS_HANDLE);
    let callback = crate::closure::js_closure_alloc(test_timer_capture_arg as *const u8, 0);
    let arg = test_string_value(b"timer-tick-arg");
    let timer_args = [arg];
    let timer_id = unsafe {
        crate::timer::js_set_timeout_callback_args(callback as i64, 0.0, timer_args.as_ptr(), 1)
    };

    let previous = test_string_value(b"timer-previous-context");
    let previous_original = (previous.to_bits() & POINTER_MASK) as usize;
    crate::async_context::enter_with(ALS_HANDLE, previous);
    TEST_TIMER_ARG_BITS.with(|slot| slot.set(0));
    TEST_TIMER_CALLED.with(|slot| slot.set(false));

    let before = gc_collection_count();
    assert_eq!(crate::timer::js_callback_timer_tick(), 1);
    assert!(
        gc_collection_count() > before,
        "timer before/after hooks should trigger copied-minor GC"
    );
    assert!(TEST_TIMER_CALLED.with(|slot| slot.get()));

    let restored = crate::async_context::get_store(ALS_HANDLE)
        .expect("timer tick should restore previous AsyncLocalStorage context");
    assert_moved_string_value(restored, previous_original, b"timer-previous-context");
    crate::async_context::clear_store(ALS_HANDLE);
    crate::timer::clearTimeout(timer_id);
}

#[test]
fn test_queued_microtask_previous_context_survives_hook_gc() {
    const ALS_HANDLE: i64 = -8_502;

    let _async_hook_guard = AsyncHookRuntimeTestGuard::new();
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);
    gc_register_mutable_root_scanner(crate::async_hooks::scan_async_hooks_roots_mut);
    gc_register_mutable_root_scanner(crate::builtins::scan_queued_microtask_roots_mut);

    let before_hook =
        crate::closure::js_closure_alloc(test_async_hook_event_force_minor_gc as *const u8, 0);
    enable_async_hook(&[(b"before", before_hook)]);

    crate::async_context::clear_store(ALS_HANDLE);
    let callback = crate::closure::js_closure_alloc(test_no_capture_singleton_func as *const u8, 0);
    crate::builtins::js_queue_microtask(callback as i64);

    let previous = test_string_value(b"microtask-previous-context");
    let previous_original = (previous.to_bits() & POINTER_MASK) as usize;
    crate::async_context::enter_with(ALS_HANDLE, previous);

    let before = gc_collection_count();
    crate::builtins::js_drain_queued_microtasks();
    assert!(
        gc_collection_count() > before,
        "queued microtask before hook should trigger copied-minor GC"
    );

    let restored = crate::async_context::get_store(ALS_HANDLE)
        .expect("queued microtask should restore previous AsyncLocalStorage context");
    assert_moved_string_value(restored, previous_original, b"microtask-previous-context");
    crate::async_context::clear_store(ALS_HANDLE);
}

#[test]
fn test_array_map_runtime_handles_survive_callback_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let input = test_string_value(b"array-map-payload");
    let input_ptr = (input.to_bits() & POINTER_MASK) as usize;
    let source = test_array_from_values(&[input]);
    let callback =
        crate::closure::js_closure_alloc(test_array_identity_force_minor_gc as *const u8, 0);

    let before = gc_collection_count();
    let result = crate::array::js_array_map(source, callback);
    assert!(
        gc_collection_count() > before,
        "array.map callback should force copied-minor GC while result is runtime-held"
    );
    assert_eq!(crate::array::js_array_length(result), 1);

    let stored = crate::array::js_array_get(result, 0).bits();
    assert_eq!(stored & TAG_MASK, STRING_TAG);
    let stored_ptr = (stored & POINTER_MASK) as *const crate::StringHeader;
    assert_ne!(
        stored_ptr as usize, input_ptr,
        "mapped heap value should be rewritten to its copied-minor address"
    );
    unsafe {
        assert_string_bytes(stored_ptr, b"array-map-payload");
    }
}

#[test]
fn test_map_materializers_runtime_handles_survive_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let key = test_string_value(b"map-key");
    let key_original = (key.to_bits() & POINTER_MASK) as usize;
    let value = test_string_value(b"map-value");
    let value_original = (value.to_bits() & POINTER_MASK) as usize;
    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, key, value);
    let map_scope = RuntimeHandleScope::new();
    let map_handle = map_scope.root_raw_mut_ptr(map);

    let before = gc_collection_count();
    crate::map::test_force_next_map_helper_gc();
    let entries = crate::map::js_map_entries(map_handle.get_raw_const_ptr());
    assert!(
        gc_collection_count() > before,
        "Map.entries should force copied-minor GC while helper handles are live"
    );
    assert_eq!(crate::array::js_array_length(entries), 1);
    let pair_bits = crate::array::js_array_get(entries, 0).bits();
    assert_eq!(pair_bits & TAG_MASK, POINTER_TAG);
    let pair = (pair_bits & POINTER_MASK) as *mut crate::array::ArrayHeader;
    assert_moved_string_value(
        crate::array::js_array_get_f64(pair, 0),
        key_original,
        b"map-key",
    );
    assert_moved_string_value(
        crate::array::js_array_get_f64(pair, 1),
        value_original,
        b"map-value",
    );

    crate::map::test_force_next_map_helper_gc();
    let keys = crate::map::js_map_keys(map_handle.get_raw_const_ptr());
    assert_eq!(crate::array::js_array_length(keys), 1);
    assert_moved_string_value(
        crate::array::js_array_get_f64(keys, 0),
        key_original,
        b"map-key",
    );

    crate::map::test_force_next_map_helper_gc();
    let values = crate::map::js_map_values(map_handle.get_raw_const_ptr());
    assert_eq!(crate::array::js_array_length(values), 1);
    assert_moved_string_value(
        crate::array::js_array_get_f64(values, 0),
        value_original,
        b"map-value",
    );
}

#[test]
fn test_map_from_array_runtime_handles_survive_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let key = test_string_value(b"map-from-array-key");
    let key_original = (key.to_bits() & POINTER_MASK) as usize;
    let value = test_string_value(b"map-from-array-value");
    let value_original = (value.to_bits() & POINTER_MASK) as usize;
    let pair = test_pair_array(key, value);
    let input = test_array_from_pair(pair);

    let before = gc_collection_count();
    crate::map::test_force_next_map_helper_gc();
    let map = crate::map::js_map_from_array(input);
    assert!(
        gc_collection_count() > before,
        "Map-from-array should force copied-minor GC while input and output handles are live"
    );
    assert_eq!(crate::map::js_map_size(map), 1);
    assert_moved_string_value(
        crate::map::js_map_entry_key_at(map, 0),
        key_original,
        b"map-from-array-key",
    );
    assert_moved_string_value(
        crate::map::js_map_entry_value_at(map, 0),
        value_original,
        b"map-from-array-value",
    );
}

#[test]
fn test_structured_clone_map_runtime_handles_survive_nested_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let outer_key = test_string_value(b"clone-map-outer-key");
    let outer_key_original = (outer_key.to_bits() & POINTER_MASK) as usize;
    let inner_key = test_string_value(b"clone-map-inner-key");
    let inner_key_original = (inner_key.to_bits() & POINTER_MASK) as usize;
    let inner_value = test_string_value(b"clone-map-inner-value");
    let inner_value_original = (inner_value.to_bits() & POINTER_MASK) as usize;

    let inner_map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(inner_map, inner_key, inner_value);
    let outer_map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(
        outer_map,
        outer_key,
        f64::from_bits(ptr_bits(inner_map as usize)),
    );

    let before = gc_collection_count();
    crate::map::test_force_next_map_helper_gc();
    crate::map::test_force_next_map_helper_gc();
    let cloned = crate::builtins::js_structured_clone(f64::from_bits(ptr_bits(outer_map as usize)));
    assert!(
        gc_collection_count() >= before + 2,
        "structuredClone(Map) should force copied-minor GC in outer and nested Map.entries"
    );

    let cloned_bits = cloned.to_bits();
    assert_eq!(cloned_bits & TAG_MASK, POINTER_TAG);
    let cloned_map = (cloned_bits & POINTER_MASK) as *mut crate::map::MapHeader;
    assert!(crate::map::is_registered_map(cloned_map as usize));
    assert_eq!(crate::map::js_map_size(cloned_map), 1);
    assert_moved_string_value(
        crate::map::js_map_entry_key_at(cloned_map, 0),
        outer_key_original,
        b"clone-map-outer-key",
    );

    let inner_clone_value = crate::map::js_map_entry_value_at(cloned_map, 0);
    let inner_clone_bits = inner_clone_value.to_bits();
    assert_eq!(inner_clone_bits & TAG_MASK, POINTER_TAG);
    let inner_clone = (inner_clone_bits & POINTER_MASK) as *mut crate::map::MapHeader;
    assert!(crate::map::is_registered_map(inner_clone as usize));
    assert_ne!(inner_clone as usize, inner_map as usize);
    assert_eq!(crate::map::js_map_size(inner_clone), 1);
    assert_moved_string_value(
        crate::map::js_map_entry_key_at(inner_clone, 0),
        inner_key_original,
        b"clone-map-inner-key",
    );
    assert_moved_string_value(
        crate::map::js_map_entry_value_at(inner_clone, 0),
        inner_value_original,
        b"clone-map-inner-value",
    );
}

#[test]
fn test_set_materializers_runtime_handles_survive_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let value = test_string_value(b"set-value");
    let value_original = (value.to_bits() & POINTER_MASK) as usize;
    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, value);

    let before = gc_collection_count();
    crate::set::test_force_next_set_helper_gc();
    let arr = crate::set::js_set_to_array(set);
    assert!(
        gc_collection_count() > before,
        "Set-to-array should force copied-minor GC while helper handles are live"
    );
    assert_eq!(crate::array::js_array_length(arr), 1);
    assert_moved_string_value(
        crate::array::js_array_get_f64(arr, 0),
        value_original,
        b"set-value",
    );

    let from_array_value = test_string_value(b"set-from-array");
    let from_array_original = (from_array_value.to_bits() & POINTER_MASK) as usize;
    let input = test_array_from_values(&[from_array_value]);
    crate::set::test_force_next_set_helper_gc();
    let from_array = crate::set::js_set_from_array(input);
    assert_eq!(crate::set::js_set_size(from_array), 1);
    assert_moved_string_value(
        crate::set::js_set_value_at(from_array, 0),
        from_array_original,
        b"set-from-array",
    );

    let iterable = test_string_value(b"ab");
    crate::set::test_force_next_set_helper_gc();
    let from_iterable = crate::set::js_set_from_iterable(iterable);
    assert_eq!(crate::set::js_set_size(from_iterable), 2);
    unsafe {
        let first = crate::set::js_set_value_at(from_iterable, 0);
        let second = crate::set::js_set_value_at(from_iterable, 1);
        assert_string_bytes(
            (first.to_bits() & POINTER_MASK) as *const crate::StringHeader,
            b"a",
        );
        assert_string_bytes(
            (second.to_bits() & POINTER_MASK) as *const crate::StringHeader,
            b"b",
        );
    }
}
