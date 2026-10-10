//! Named-array accessor rebinding must root both halves and the live owner.
use super::*;

extern "C" fn array_capture_getter(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let cell = (crate::closure::js_closure_get_capture_bits(closure, 0) & POINTER_MASK)
        as *mut crate::r#box::Box;
    let owner = (crate::closure::js_closure_get_capture_bits(closure, 1) & POINTER_MASK)
        as *const crate::array::ArrayHeader;
    crate::r#box::js_box_get(cell) + f64::from(crate::array::js_array_length(owner))
}

extern "C" fn array_capture_setter(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let cell = (crate::closure::js_closure_get_capture_bits(closure, 0) & POINTER_MASK)
        as *mut crate::r#box::Box;
    let owner = (crate::closure::js_closure_get_capture_bits(closure, 1) & POINTER_MASK)
        as *const crate::array::ArrayHeader;
    crate::r#box::js_box_set(
        cell,
        value + f64::from(crate::array::js_array_length(owner)),
    );
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// The allocation counter is gated by the runtime's cached profiling flag.
/// Enable the existing instrumentation for this single-threaded witness and
/// restore its prior state even when an assertion unwinds.
struct ClosureCountGuard(bool);

impl ClosureCountGuard {
    fn new() -> Self {
        Self(crate::promise::MT_PROFILE_ENABLED.swap(true, std::sync::atomic::Ordering::Relaxed))
    }
}

impl Drop for ClosureCountGuard {
    fn drop(&mut self) {
        crate::promise::MT_PROFILE_ENABLED.store(self.0, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Fill the current nursery block with initialized, dead GC arrays, leaving
/// exactly the selected number of clone allocations. Naked arena bytes are
/// not padding: every walker must see a valid object header and payload.
/// Trigger arming happens AFTER padding, and no padding allocation collects.
unsafe fn leave_rebind_space(clones: usize) -> usize {
    let bytes = (GC_HEADER_SIZE + crate::closure::closure_payload_size(2) + 7) & !7;
    let cycles = copying_minor_cycles();
    crate::array::js_array_alloc(0);
    // The inline Eden offset is authoritative; publish it before reading the block.
    crate::arena::sync_inline_arena_state();
    let remaining = {
        let arena = &*crate::arena::hot_arena();
        let block = &arena.blocks[arena.current];
        block.size - block.offset
    };
    assert!(remaining >= clones * bytes);
    let mut padding = remaining - clones * bytes;
    let minimum = GC_HEADER_SIZE + std::mem::size_of::<crate::array::ArrayHeader>();
    while padding != 0 {
        assert!(padding >= minimum, "padding must fit a complete GC array");
        let mut total = padding.min(crate::gc::LARGE_OBJECT_THRESHOLD_BYTES);
        if padding - total != 0 && padding - total < minimum {
            total -= minimum;
        }
        let payload = total - GC_HEADER_SIZE;
        let raw = crate::arena::arena_alloc_gc_no_collect(payload, 8, GC_TYPE_ARRAY);
        assert!(!raw.is_null(), "padding must use the current nursery block");
        let arr = raw as *mut crate::array::ArrayHeader;
        let capacity = (payload - std::mem::size_of::<crate::array::ArrayHeader>()) / 8;
        (*arr).length = 0;
        (*arr).capacity = capacity as u32;
        let slots = arr.add(1) as *mut u64;
        for index in 0..capacity {
            slots.add(index).write(crate::value::TAG_HOLE);
        }
        crate::gc::layout_init_pointer_free(raw);
        padding -= total;
    }
    assert_eq!(
        copying_minor_cycles(),
        cycles,
        "padding is outside the clone window"
    );
    crate::arena::sync_inline_arena_state();
    let arena = &*crate::arena::hot_arena();
    let block = &arena.blocks[arena.current];
    assert_eq!(block.size - block.offset, clones * bytes);
    block.data.add(block.offset + GC_HEADER_SIZE) as usize
}

unsafe fn assert_rebind_source(handle: &crate::gc::RuntimeHandle<'_>) {
    let header = addr_of(handle.get_nanbox_f64()) as *const crate::closure::ClosureHeader;
    assert_eq!(
        (*header).capture_count,
        crate::closure::CAPTURES_THIS_FLAG | 2
    );
    assert!(crate::closure::closure_reads_this_from_capture(header));
}

unsafe fn accessor_bag<'a>(
    scope: &'a RuntimeHandleScope,
    get: Option<f64>,
    set: Option<f64>,
) -> crate::gc::RuntimeHandle<'a> {
    let bag = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    for (name, value) in [("get", get), ("set", set)] {
        if let Some(value) = value {
            crate::object::js_object_set_property_key(
                bag.get_nanbox_f64(),
                string_value(name),
                value,
            );
        }
    }
    bag
}

unsafe fn check_array_pair(
    receiver: &crate::gc::RuntimeHandle<'_>,
    cell: &crate::gc::RuntimeHandle<'_>,
    name: &str,
) -> crate::object::AccessorDescriptor {
    let pair = crate::object::get_accessor_descriptor(addr_of(receiver.get_nanbox_f64()), name)
        .expect("accessor metadata must be filed at the refreshed owner");
    for bits in [pair.get, pair.set] {
        assert_ne!(bits, 0);
        let closure = (bits & POINTER_MASK) as *const crate::closure::ClosureHeader;
        assert_eq!(
            (*closure).capture_count,
            crate::closure::CAPTURES_THIS_FLAG | 2
        );
        assert_eq!(
            crate::closure::js_closure_get_capture_bits(closure, 0),
            cell.get_nanbox_u64()
        );
        assert_eq!(
            crate::closure::js_closure_get_capture_bits(closure, 1),
            receiver.get_nanbox_u64()
        );
    }
    assert_eq!(
        (*(*((pair.get & POINTER_MASK) as *const crate::closure::ClosureHeader)).info).code,
        array_capture_getter as *const u8
    );
    assert_eq!(
        (*(*((pair.set & POINTER_MASK) as *const crate::closure::ClosureHeader)).info).code,
        array_capture_setter as *const u8
    );
    let attrs =
        crate::object::get_property_attrs(addr_of(receiver.get_nanbox_f64()), name).unwrap();
    assert!(attrs.enumerable());
    assert!(attrs.configurable());
    assert!(!attrs.writable());
    assert_eq!(read_property(receiver.get_nanbox_f64(), name), 17.0);
    let key = string_value(name);
    crate::array::js_array_set_string_key(
        addr_of(receiver.get_nanbox_f64()) as *mut crate::array::ArrayHeader,
        string_ptr_of(key),
        29.0,
    );
    assert_eq!(
        crate::r#box::js_box_get(addr_of(cell.get_nanbox_f64()) as *mut crate::r#box::Box),
        29.0
    );
    assert_eq!(read_property(receiver.get_nanbox_f64(), name), 29.0);
    pair
}

#[test]
fn descriptor_snapshot_named_array_accessors_copy_in_each_rebind_window() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _counts = ClosureCountGuard::new();
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::object::canonical_keys::scan_canonical_keys_roots_mut);
    gc_register_mutable_root_scanner(crate::string::scan_intern_table_roots_mut);
    super::super::dead_owner_side_tables::register_array_side_table_scanners();
    unsafe {
        // Call the decoded consumer directly: public current-record builders
        // and proxy trap lookup may allocate before the actual clone window.
        // Their observable merge behavior has separate public-entry coverage.
        for setter_window in [false, true] {
            let trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
            let scope = RuntimeHandleScope::new();
            let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::array::js_array_alloc(0) as i64,
            ));
            let cell = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::r#box::js_box_alloc_bits(17.0f64.to_bits() as i64) as i64,
            ));
            let captures = [cell.get_nanbox_u64(), crate::value::TAG_UNDEFINED];
            let get = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::closure::js_closure_alloc_init_boxed(
                    crate::fn_info!(array_capture_getter, 0),
                    crate::closure::CAPTURES_THIS_FLAG | 2,
                    captures.as_ptr(),
                ) as i64,
            ));
            let set = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::closure::js_closure_alloc_init_boxed(
                    crate::fn_info!(array_capture_setter, 1),
                    crate::closure::CAPTURES_THIS_FLAG | 2,
                    captures.as_ptr(),
                ) as i64,
            ));
            let key = scope.root_nanbox_f64(string_value("rebind_named"));
            let bag = accessor_bag(
                &scope,
                Some(get.get_nanbox_f64()),
                Some(set.get_nanbox_f64()),
            );
            for flag in ["enumerable", "configurable"] {
                crate::object::js_object_set_property_key(
                    bag.get_nanbox_f64(),
                    string_value(flag),
                    f64::from_bits(crate::value::TAG_TRUE),
                );
            }
            let view = crate::object::object_ops::decode_property_descriptor(&scope, &bag);
            crate::object::js_object_get_own_property_descriptor(
                receiver.get_nanbox_f64(),
                key.get_nanbox_f64(),
            );
            let before_receiver = addr_of(receiver.get_nanbox_f64());
            let before_get = addr_of(get.get_nanbox_f64());
            let before_set = addr_of(set.get_nanbox_f64());
            assert_rebind_source(&get);
            assert_rebind_source(&set);
            let expected_first_get = leave_rebind_space(usize::from(setter_window));
            let cycles = copying_minor_cycles();
            let moved = moved_objects_total();
            let allocations =
                crate::closure::CLOSURE_ALLOC_COUNT.load(std::sync::atomic::Ordering::Relaxed);
            trigger.make_arena_trigger_due();
            assert_eq!(
                crate::object::define_array_property(
                    addr_of(receiver.get_nanbox_f64()) as *mut crate::object::ObjectHeader,
                    receiver.get_nanbox_f64(),
                    string_ptr_of(key.get_nanbox_f64()),
                    Some("rebind_named"),
                    &view
                ),
                Some(true)
            );
            assert_eq!(
                crate::closure::CLOSURE_ALLOC_COUNT.load(std::sync::atomic::Ordering::Relaxed)
                    - allocations,
                2
            );
            assert_eq!(
                copying_minor_cycles() - cycles,
                1,
                "one collection belongs to the selected rebind window"
            );
            assert!(moved_objects_total() > moved);
            assert_ne!(addr_of(receiver.get_nanbox_f64()), before_receiver);
            assert_ne!(addr_of(get.get_nanbox_f64()), before_get);
            assert_ne!(addr_of(set.get_nanbox_f64()), before_set);
            let pair = crate::object::get_accessor_descriptor(
                addr_of(receiver.get_nanbox_f64()),
                "rebind_named",
            )
            .unwrap();
            assert_ne!(pair.get, get.get_nanbox_u64(), "getter must really clone");
            assert_ne!(pair.set, set.get_nanbox_u64(), "setter must really clone");
            if setter_window {
                // This deliberately inspected from-space header is test
                // evidence only. The default unit arm does not protect it.
                // It proves the first new getter existed before the minor
                // in the second clone and was relocated to the installed
                // getter. No test handle independently retains that clone.
                let header = header_from_user_ptr(expected_first_get as *const u8);
                assert_eq!((*header).obj_type, GC_TYPE_CLOSURE);
                assert_ne!((*header).gc_flags & GC_FLAG_FORWARDED, 0);
                assert_eq!(
                    forwarding_address(header) as usize,
                    (pair.get & POINTER_MASK) as usize
                );
            }
            check_array_pair(&receiver, &cell, "rebind_named");
        }
    }
}

#[test]
fn descriptor_snapshot_named_array_retained_accessor_halves_survive_rebind_copying() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _counts = ClosureCountGuard::new();
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::object::canonical_keys::scan_canonical_keys_roots_mut);
    gc_register_mutable_root_scanner(crate::string::scan_intern_table_roots_mut);
    super::super::dead_owner_side_tables::register_array_side_table_scanners();
    unsafe {
        for replace_get in [false, true] {
            let trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
            let scope = RuntimeHandleScope::new();
            let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::array::js_array_alloc(0) as i64,
            ));
            let cell = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::r#box::js_box_alloc_bits(17.0f64.to_bits() as i64) as i64,
            ));
            let captures = [cell.get_nanbox_u64(), crate::value::TAG_UNDEFINED];
            let get = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::closure::js_closure_alloc_init_boxed(
                    crate::fn_info!(array_capture_getter, 0),
                    crate::closure::CAPTURES_THIS_FLAG | 2,
                    captures.as_ptr(),
                ) as i64,
            ));
            let set = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::closure::js_closure_alloc_init_boxed(
                    crate::fn_info!(array_capture_setter, 1),
                    crate::closure::CAPTURES_THIS_FLAG | 2,
                    captures.as_ptr(),
                ) as i64,
            ));
            let key = scope.root_nanbox_f64(string_value("retained_named"));
            let initial = accessor_bag(
                &scope,
                Some(get.get_nanbox_f64()),
                Some(set.get_nanbox_f64()),
            );
            for flag in ["enumerable", "configurable"] {
                crate::object::js_object_set_property_key(
                    initial.get_nanbox_f64(),
                    string_value(flag),
                    f64::from_bits(crate::value::TAG_TRUE),
                );
            }
            crate::object::js_object_define_property(
                receiver.get_nanbox_f64(),
                key.get_nanbox_f64(),
                initial.get_nanbox_f64(),
            );
            let prior = crate::object::get_accessor_descriptor(
                addr_of(receiver.get_nanbox_f64()),
                "retained_named",
            )
            .unwrap();
            // Expected identity observations are rewritten independently; they
            // cannot refresh the consumer's saved locals or installed words.
            // The critical newly returned getter has no such test root.
            let expected_get = scope.root_nanbox_u64(prior.get);
            let expected_set = scope.root_nanbox_u64(prior.set);
            let bag = accessor_bag(
                &scope,
                replace_get.then(|| get.get_nanbox_f64()),
                (!replace_get).then(|| set.get_nanbox_f64()),
            );
            let view = crate::object::object_ops::decode_property_descriptor(&scope, &bag);
            let before_receiver = addr_of(receiver.get_nanbox_f64());
            assert_rebind_source(&get);
            assert_rebind_source(&set);
            leave_rebind_space(0);
            let cycles = copying_minor_cycles();
            let moved = moved_objects_total();
            let allocations =
                crate::closure::CLOSURE_ALLOC_COUNT.load(std::sync::atomic::Ordering::Relaxed);
            trigger.make_arena_trigger_due();
            // Direct decoded application isolates the rebind window for an
            // existing key from the public current-record builder allocation.
            assert_eq!(
                crate::object::define_array_property(
                    addr_of(receiver.get_nanbox_f64()) as *mut crate::object::ObjectHeader,
                    receiver.get_nanbox_f64(),
                    string_ptr_of(key.get_nanbox_f64()),
                    Some("retained_named"),
                    &view
                ),
                Some(true)
            );
            assert_eq!(copying_minor_cycles() - cycles, 1);
            assert!(moved_objects_total() > moved);
            assert_ne!(addr_of(receiver.get_nanbox_f64()), before_receiver);
            assert_eq!(
                crate::closure::CLOSURE_ALLOC_COUNT.load(std::sync::atomic::Ordering::Relaxed)
                    - allocations,
                1
            );
            let pair = crate::object::get_accessor_descriptor(
                addr_of(receiver.get_nanbox_f64()),
                "retained_named",
            )
            .unwrap();
            let retained_before = if replace_get { prior.set } else { prior.get };
            let retained_after = if replace_get { pair.set } else { pair.get };
            assert_ne!(
                retained_after, retained_before,
                "the omitted prior half must relocate"
            );
            let expected = if replace_get {
                expected_set.get_nanbox_u64()
            } else {
                expected_get.get_nanbox_u64()
            };
            assert_eq!(
                retained_after, expected,
                "omitted half retains its exact live identity"
            );
            check_array_pair(&receiver, &cell, "retained_named");
        }
    }
}

#[test]
fn descriptor_snapshot_named_array_rebind_merge_through_public_entries() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    unsafe {
        for proxied in [false, true] {
            for reflect in [false, true] {
                let scope = RuntimeHandleScope::new();
                let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                    crate::array::js_array_alloc(0) as i64,
                ));
                let cell = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                    crate::r#box::js_box_alloc_bits(17.0f64.to_bits() as i64) as i64,
                ));
                let captures = [cell.get_nanbox_u64(), crate::value::TAG_UNDEFINED];
                let get = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                    crate::closure::js_closure_alloc_init_boxed(
                        crate::fn_info!(array_capture_getter, 0),
                        crate::closure::CAPTURES_THIS_FLAG | 2,
                        captures.as_ptr(),
                    ) as i64,
                ));
                let set = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                    crate::closure::js_closure_alloc_init_boxed(
                        crate::fn_info!(array_capture_setter, 1),
                        crate::closure::CAPTURES_THIS_FLAG | 2,
                        captures.as_ptr(),
                    ) as i64,
                ));
                let key = scope.root_nanbox_f64(string_value("public_named"));
                let target = scope.root_nanbox_f64(if proxied {
                    let handler =
                        scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
                    crate::proxy::js_proxy_new(receiver.get_nanbox_f64(), handler.get_nanbox_f64())
                } else {
                    receiver.get_nanbox_f64()
                });
                let define = |bag: &crate::gc::RuntimeHandle<'_>| {
                    if reflect {
                        assert_eq!(
                            crate::proxy::js_reflect_define_property(
                                target.get_nanbox_f64(),
                                key.get_nanbox_f64(),
                                bag.get_nanbox_f64()
                            )
                            .to_bits(),
                            crate::value::TAG_TRUE
                        );
                    } else {
                        crate::object::js_object_define_property(
                            target.get_nanbox_f64(),
                            key.get_nanbox_f64(),
                            bag.get_nanbox_f64(),
                        );
                    }
                };
                let bag = accessor_bag(
                    &scope,
                    Some(get.get_nanbox_f64()),
                    Some(set.get_nanbox_f64()),
                );
                for flag in ["enumerable", "configurable"] {
                    crate::object::js_object_set_property_key(
                        bag.get_nanbox_f64(),
                        string_value(flag),
                        f64::from_bits(crate::value::TAG_TRUE),
                    );
                }
                define(&bag);
                let initial = crate::object::get_accessor_descriptor(
                    addr_of(receiver.get_nanbox_f64()),
                    "public_named",
                )
                .unwrap();
                define(&accessor_bag(&scope, Some(get.get_nanbox_f64()), None));
                let after_get = crate::object::get_accessor_descriptor(
                    addr_of(receiver.get_nanbox_f64()),
                    "public_named",
                )
                .unwrap();
                assert_eq!(
                    after_get.set, initial.set,
                    "omitted setter retains its rebound identity"
                );
                define(&accessor_bag(&scope, None, Some(set.get_nanbox_f64())));
                let after_set = crate::object::get_accessor_descriptor(
                    addr_of(receiver.get_nanbox_f64()),
                    "public_named",
                )
                .unwrap();
                assert_eq!(
                    after_set.get, after_get.get,
                    "omitted getter retains its rebound identity"
                );
                let generic = accessor_bag(&scope, None, None);
                crate::object::js_object_set_property_key(
                    generic.get_nanbox_f64(),
                    string_value("enumerable"),
                    f64::from_bits(crate::value::TAG_TRUE),
                );
                define(&generic);
                let after_generic = crate::object::get_accessor_descriptor(
                    addr_of(receiver.get_nanbox_f64()),
                    "public_named",
                )
                .unwrap();
                assert_eq!(after_generic.get, after_set.get);
                assert_eq!(after_generic.set, after_set.set);
                check_array_pair(&receiver, &cell, "public_named");
                define(&accessor_bag(
                    &scope,
                    Some(f64::from_bits(crate::value::TAG_UNDEFINED)),
                    None,
                ));
                let cleared_get = crate::object::get_accessor_descriptor(
                    addr_of(receiver.get_nanbox_f64()),
                    "public_named",
                )
                .unwrap();
                assert_eq!(cleared_get.get, 0);
                assert_eq!(cleared_get.set, after_set.set);
                assert_eq!(
                    read_property(receiver.get_nanbox_f64(), "public_named").to_bits(),
                    crate::value::TAG_UNDEFINED
                );
                define(&accessor_bag(
                    &scope,
                    Some(get.get_nanbox_f64()),
                    Some(f64::from_bits(crate::value::TAG_UNDEFINED)),
                ));
                let cleared_set = crate::object::get_accessor_descriptor(
                    addr_of(receiver.get_nanbox_f64()),
                    "public_named",
                )
                .unwrap();
                assert_ne!(cleared_set.get, 0);
                assert_eq!(cleared_set.set, 0);
                assert_eq!(
                    read_property(receiver.get_nanbox_f64(), "public_named"),
                    29.0
                );
                let attrs = crate::object::get_property_attrs(
                    addr_of(receiver.get_nanbox_f64()),
                    "public_named",
                )
                .unwrap();
                assert!(attrs.enumerable());
                assert!(attrs.configurable());
            }
        }
    }
}
