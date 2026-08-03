use super::*;

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
    // `nursery_user` is live across the `arena_alloc_gc_old` call below (used
    // afterwards to build `nursery_hdr`); the block-full slow path in that
    // allocation can reach `gc_check_trigger()`, so suppress automatic
    // triggers for the setup below.
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
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
fn test_class_side_table_scanner_marks_values_but_not_function_keys() {
    let _guard = GcTestIsolationGuard::new();
    // `dynamic_value`/`prototype_value`/`cached_value`/`prototype_object` are
    // all live across the two `arena_alloc_gc` calls below (`parent_closure`,
    // `function_key`), and `parent_closure` is live across `function_key`'s —
    // any of those allocations can reach the block-full slow path's
    // `gc_check_trigger()`.
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_marks();
    clear_mark_seeds();
    crate::object::test_clear_class_side_table_roots();

    let dynamic_value = young_leaf();
    let prototype_value = young_leaf();
    let cached_value = young_leaf();
    let prototype_object = crate::object::js_object_alloc(0, 0) as usize;
    let parent_closure = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    ) as usize;
    let function_key = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    ) as usize;
    unsafe {
        init_test_closure(parent_closure as *mut u8);
        init_test_closure(function_key as *mut u8);
    }

    crate::object::test_seed_class_dynamic_prop_root(0x5201, "dyn", string_bits(dynamic_value));
    crate::object::test_seed_class_prototype_method_root(
        0x5201,
        "proto",
        string_bits(prototype_value),
    );
    crate::object::test_seed_class_prototype_method_value_root(
        0x5201,
        "bound",
        string_bits(cached_value),
    );
    crate::object::test_seed_class_prototype_object_root(0x5201, prototype_object);
    crate::object::test_seed_class_parent_closure_root(0x5201, parent_closure);
    crate::object::test_seed_function_class_id_key(ptr_bits(function_key), 0x8200_5201);

    let valid_ptrs = build_valid_pointer_set();
    crate::object::scan_class_side_table_roots_mut(&mut RuntimeRootVisitor::for_mark(&valid_ptrs));

    assert_marked_user_ptr(dynamic_value, "dynamic class property value");
    assert_marked_user_ptr(prototype_value, "prototype method value");
    assert_marked_user_ptr(cached_value, "cached bound prototype method value");
    assert_marked_user_ptr(prototype_object, "prototype-object side-table value");
    assert_marked_user_ptr(parent_closure, "parent-closure side-table value");
    assert_unmarked_user_ptr(function_key, "function-to-class metadata key");

    crate::object::test_clear_class_side_table_roots();
    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_registered_class_side_table_scanner_rewrites_values_and_function_keys() {
    let _guard = GcTestIsolationGuard::new();
    // `value_user` is live across `key_user`'s `arena_alloc_gc` call, and both
    // `value_user`/`key_user` (plus `value_old` once allocated) are live
    // across the remaining `arena_alloc_gc`/`arena_alloc_gc_old` calls below.
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::object::test_clear_class_side_table_roots();
    gc_register_mutable_root_scanner(crate::object::scan_class_side_table_roots_mut);

    let value_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let key_user = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(key_user);
    }
    let valid_ptrs = build_valid_pointer_set();
    let value_old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let key_old = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(key_old);
        set_forwarding_address(header_from_user_ptr(value_user) as *mut GcHeader, value_old);
        set_forwarding_address(header_from_user_ptr(key_user) as *mut GcHeader, key_old);
    }

    let value_bits = ptr_bits(value_user as usize);
    let value_old_bits = ptr_bits(value_old as usize);
    let key_bits = ptr_bits(key_user as usize);
    let key_old_bits = ptr_bits(key_old as usize);
    crate::object::test_seed_class_dynamic_prop_root(0x5202, "dyn", value_bits);
    crate::object::test_seed_class_prototype_method_root(0x5202, "proto", value_bits);
    crate::object::test_seed_class_prototype_method_value_root(0x5202, "bound", value_bits);
    crate::object::test_seed_class_prototype_object_root(0x5202, value_user as usize);
    crate::object::test_seed_class_parent_closure_root(0x5202, key_user as usize);
    crate::object::test_seed_function_class_id_key(key_bits, 0x8200_5202);

    rewrite_mutable_registered_roots(&valid_ptrs);

    assert_eq!(
        crate::object::test_class_dynamic_prop_root_bits(0x5202, "dyn"),
        value_old_bits
    );
    assert_eq!(
        crate::object::test_class_prototype_method_root_bits(0x5202, "proto"),
        value_old_bits
    );
    assert_eq!(
        crate::object::test_class_prototype_method_value_root_bits(0x5202, "bound"),
        value_old_bits
    );
    assert_eq!(
        crate::object::test_class_prototype_object_root_addr(0x5202),
        value_old as usize
    );
    assert_eq!(
        crate::object::test_class_parent_closure_root_addr(0x5202),
        key_old as usize
    );
    assert_eq!(
        crate::object::function_class_id(f64::from_bits(key_old_bits)),
        0x8200_5202
    );
    assert_eq!(
        crate::object::test_function_class_id_key_for_class(0x8200_5202),
        key_old_bits
    );
    assert_eq!(
        crate::object::function_class_id(f64::from_bits(key_bits)),
        0
    );

    crate::object::test_clear_class_side_table_roots();
}

#[test]
fn test_symbol_side_table_scanner_marks_keys_and_values_without_marking_owner() {
    // Not exposed: every allocation here goes through opaque helpers
    // (`js_object_alloc`, `alloc_nursery_test_symbol`, `young_leaf`) with no
    // direct `arena_alloc_gc`/`arena_alloc_gc_old` call written in this file,
    // so there is no in-file call site to guard.
    let _guard = GcTestIsolationGuard::new();
    clear_marks();
    clear_mark_seeds();
    crate::symbol::test_clear_symbol_side_table_roots();

    let owner = crate::object::js_object_alloc(0, 0) as usize;
    let sym_key = unsafe { alloc_nursery_test_symbol() };
    let value = young_leaf();
    let static_sym_key = unsafe { alloc_nursery_test_symbol() };
    let static_value = young_leaf();

    crate::symbol::test_seed_symbol_property_root(owner, sym_key, string_bits(value));
    crate::symbol::test_seed_class_static_symbol_root(
        0x5301,
        static_sym_key,
        string_bits(static_value),
    );

    let valid_ptrs = build_valid_pointer_set();
    crate::symbol::scan_symbol_side_table_roots_mut(&mut RuntimeRootVisitor::for_mark(&valid_ptrs));

    assert_unmarked_user_ptr(owner, "symbol side-table owner metadata key");
    assert_marked_user_ptr(sym_key, "symbol property key");
    assert_marked_user_ptr(value, "symbol property value");
    assert_marked_user_ptr(static_sym_key, "class static symbol key");
    assert_marked_user_ptr(static_value, "class static symbol value");

    crate::symbol::test_clear_symbol_side_table_roots();
    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_symbol_side_table_registered_scanner_rewrites_roots_and_metadata() {
    let _guard = GcTestIsolationGuard::new();
    // `owner`/`sym_key`/`value`/`static_sym_key`/`static_value` (and the
    // `_old` addresses as they're allocated) are all live across the three
    // `arena_alloc_gc_old` calls below.
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::symbol::test_clear_symbol_side_table_roots();
    gc_register_mutable_root_scanner(crate::symbol::scan_symbol_side_table_roots_mut);

    let owner = crate::object::js_object_alloc(0, 0) as usize;
    let sym_key = unsafe { alloc_nursery_test_symbol() };
    let value = young_leaf();
    let static_sym_key = unsafe { alloc_nursery_test_symbol() };
    let static_value = young_leaf();

    let valid_ptrs = build_valid_pointer_set();
    let owner_old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT) as usize;
    let sym_key_old = unsafe { alloc_old_test_symbol() };
    let value_old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_STRING) as usize;
    let static_sym_key_old = unsafe { alloc_old_test_symbol() };
    let static_value_old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_STRING) as usize;
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(owner as *const u8) as *mut GcHeader,
            owner_old as *mut u8,
        );
        set_forwarding_address(
            header_from_user_ptr(sym_key as *const u8) as *mut GcHeader,
            sym_key_old as *mut u8,
        );
        set_forwarding_address(
            header_from_user_ptr(value as *const u8) as *mut GcHeader,
            value_old as *mut u8,
        );
        set_forwarding_address(
            header_from_user_ptr(static_sym_key as *const u8) as *mut GcHeader,
            static_sym_key_old as *mut u8,
        );
        set_forwarding_address(
            header_from_user_ptr(static_value as *const u8) as *mut GcHeader,
            static_value_old as *mut u8,
        );
    }

    crate::symbol::test_seed_symbol_pointer_root(sym_key);
    crate::symbol::test_seed_symbol_pointer_root(static_sym_key);
    crate::symbol::test_seed_symbol_property_root(owner, sym_key, string_bits(value));
    crate::symbol::test_seed_class_static_symbol_root(
        0x5302,
        static_sym_key,
        string_bits(static_value),
    );

    rewrite_mutable_registered_roots(&valid_ptrs);

    assert!(
        !crate::symbol::test_symbol_property_owner_exists(owner),
        "symbol side table should remove the stale owner key"
    );
    assert_eq!(
        crate::symbol::test_symbol_property_root_bits(owner_old, sym_key_old),
        Some(string_bits(value_old))
    );
    assert_eq!(
        crate::symbol::test_class_static_symbol_root_bits(0x5302, static_sym_key_old),
        Some(string_bits(static_value_old))
    );
    assert_eq!(
        crate::symbol::test_class_static_symbol_root_bits(0x5302, static_sym_key),
        None
    );
    assert!(crate::symbol::test_symbol_pointer_root_contains(
        sym_key_old
    ));
    assert!(crate::symbol::test_symbol_pointer_root_contains(
        static_sym_key_old
    ));
    assert!(!crate::symbol::test_symbol_pointer_root_contains(sym_key));
    assert!(!crate::symbol::test_symbol_pointer_root_contains(
        static_sym_key
    ));

    crate::symbol::test_clear_symbol_side_table_roots();
}

#[test]
fn test_runtime_root_visitor_rewrites_raw_pointer_slots() {
    // `nursery_user` is live across the `arena_alloc_gc_old` call below.
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
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
        test_decl_class_prototype_root, test_seed_class_inheritance_roots,
        test_seed_class_parent_closure_root, test_seed_decl_class_prototype_root,
    };

    const PROTO_CID: u32 = 0xDEAD_0001;
    const CLOSURE_CID: u32 = 0xDEAD_0002;

    // `proto_user`/`decl_proto_user`/`closure_user` (and the `_old` addresses
    // as they're allocated) are all live across the later `arena_alloc_gc`/
    // `arena_alloc_gc_old` calls below.
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_marks();
    clear_mark_seeds();

    // Allocate the side-table objects in the nursery before snapshotting the
    // valid-pointer set so the mark phase recognizes them as roots.
    let proto_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let decl_proto_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let closure_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let proto_old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let decl_proto_old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let closure_old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let proto_hdr = unsafe { header_from_user_ptr(proto_user) as *mut GcHeader };
    let decl_proto_hdr = unsafe { header_from_user_ptr(decl_proto_user) as *mut GcHeader };
    let closure_hdr = unsafe { header_from_user_ptr(closure_user) as *mut GcHeader };

    test_seed_class_inheritance_roots(PROTO_CID, proto_user as usize);
    test_seed_decl_class_prototype_root(PROTO_CID, decl_proto_user as usize);
    test_seed_class_parent_closure_root(CLOSURE_CID, closure_user as usize);

    // Mark phase: all side-table parents become live roots.
    scan_class_inheritance_roots_mut(&mut RuntimeRootVisitor::for_mark(&valid_ptrs));
    unsafe {
        assert_ne!(
            (*proto_hdr).gc_flags & GC_FLAG_MARKED,
            0,
            "CLASS_PROTOTYPE_OBJECTS parent must be marked as a root"
        );
        assert_ne!(
            (*decl_proto_hdr).gc_flags & GC_FLAG_MARKED,
            0,
            "CLASS_DECL_PROTOTYPE_OBJECTS prototype must be marked as a root"
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
        set_forwarding_address(decl_proto_hdr, decl_proto_old);
        set_forwarding_address(closure_hdr, closure_old);
    }
    scan_class_inheritance_roots_mut(&mut RuntimeRootVisitor::for_rewrite(&valid_ptrs));

    assert_eq!(
        test_class_prototype_object_root(PROTO_CID),
        proto_old as usize,
        "CLASS_PROTOTYPE_OBJECTS parent must be rewritten to the evacuated address"
    );
    assert_eq!(
        test_decl_class_prototype_root(PROTO_CID),
        decl_proto_old as usize,
        "CLASS_DECL_PROTOTYPE_OBJECTS prototype must be rewritten to the evacuated address"
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
    // `nursery_user` is live across the `arena_alloc_gc_old` call below.
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
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
    // `nursery_user` is live across the `arena_alloc_gc_old` call below.
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
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
