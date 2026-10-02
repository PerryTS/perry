//! Real serializer/rebuilder gates, not synthetic seed-id equality.
use super::*;
use crate::object::{field_rep, shapes, static_shapes};

const PACKED: &[u8] = b"cfwire_method\0cfwire_number\0";
const FINAL: u32 = shapes::SHAPE_ID_BASE + 0x7971;
const BASE: u32 = shapes::SHAPE_ID_BASE + 0x7972;
const REP: u64 = field_rep::REP_SPECIAL | (field_rep::REP_F64 << 2);

extern "C" fn capture_body(c: *const ClosureHeader, _this: closure::JsThis) -> f64 {
    f64::from_bits(closure::js_closure_get_capture_bits(c, 0))
}
fn info() -> *const closure::JsFunctionInfo {
    crate::fn_info!(capture_body, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE))
}
fn seed_final() {
    let entries = [static_shapes::ConstFnStaticEntry {
        slot: 0,
        info: info(),
    }];
    assert_eq!(
        static_shapes::js_shape_seed_plain_constfn(
            FINAL,
            PACKED.as_ptr(),
            PACKED.len() as u32,
            2,
            2,
            REP,
            entries.as_ptr(),
            1
        ),
        FINAL
    );
}
unsafe fn wire() -> SerializedValue {
    let scope = gc::RuntimeHandleScope::new();
    seed_final();
    assert_eq!(
        static_shapes::js_shape_seed_plain(
            BASE,
            PACKED.as_ptr(),
            PACKED.len() as u32,
            2,
            2,
            field_rep::REP_F64 << 2
        ),
        BASE
    );
    let keys = static_shapes::canonical_keys_for_names(&[b"cfwire_method", b"cfwire_number"]);
    let keys = scope.root_raw_mut_ptr(keys.arr() as *mut crate::array::ArrayHeader);
    let mut objects = Vec::new();
    for n in [17.0f64, 29.0] {
        let object = crate::object::alloc_plain::alloc_plain_record_inline_keys_stamped(
            2,
            keys.get_raw_mut_ptr(),
            BASE,
        );
        let object = scope.root_raw_mut_ptr(object);
        let birth = shapes::object_shape_descriptor(object.get_raw_mut_ptr()).unwrap();
        assert_eq!(
            shapes::object_shape_stamp(object.get_raw_mut_ptr()),
            BASE,
            "plain birth must use the installed carrier"
        );
        assert_eq!(birth.object_kind, shapes::ShapeObjectKind::Ordinary);
        assert_eq!(birth.rep, field_rep::REP_F64 << 2);
        assert_eq!(
            birth.special_constfn_mask, 0,
            "allocation must stay Any/F64"
        );
        let c = scope.root_raw_mut_ptr(closure::js_closure_alloc(info(), 1));
        closure::js_closure_set_capture_bits(c.get_raw_mut_ptr(), 0, n.to_bits());
        crate::object::store_object_field_slot(
            object.get_raw_mut_ptr(),
            0,
            JSValue::object_ptr(c.get_raw_mut_ptr::<u8>()).bits(),
        );
        crate::object::store_object_field_slot(object.get_raw_mut_ptr(), 1, n.to_bits());
        let entries = [static_shapes::ConstFnStaticEntry {
            slot: 0,
            info: info(),
        }];
        let obj = static_shapes::js_object_finalize_constfn_static(
            object.get_raw_mut_ptr::<crate::object::ObjectHeader>() as usize as u64,
            FINAL,
            PACKED.as_ptr(),
            PACKED.len() as u32,
            2,
            2,
            0,
            REP,
            entries.as_ptr(),
            1,
        ) as usize as *mut crate::object::ObjectHeader;
        assert_eq!(
            shapes::object_shape_stamp(obj),
            FINAL,
            "source must carry the real final shape"
        );
        let serialized = serialize_nanbox_for_thread(JSValue::object_ptr(obj.cast()).bits());
        assert!(
            matches!(
                &serialized,
                SerializedValue::Object {
                    final_constfn: Some(_),
                    parent_class_id: 0,
                    ..
                }
            ),
            "wire must carry scalar body facts and no ShapeId"
        );
        objects.push(serialized);
    }
    SerializedValue::Array(objects)
}
fn replay(seed_first: bool, production_seed: bool) {
    let payload = unsafe { wire() };
    let seed = shapes::worker_shape_seed();
    std::thread::spawn(move || unsafe {
        if seed_first {
            seed_final();
        }
        if production_seed {
            shapes::install_worker_shape_seed(&seed);
        }
        let bits = deserialize_nanbox_on_current_thread(&payload);
        let array = JSValue::from_bits(bits).as_pointer::<crate::array::ArrayHeader>();
        let scope = gc::RuntimeHandleScope::new();
        let array = scope.root_raw_mut_ptr(array as *mut crate::array::ArrayHeader);
        let mut ids = Vec::new();
        let mut cells = Vec::new();
        for (slot, expected) in [17.0, 29.0].into_iter().enumerate() {
            let object = JSValue::from_bits(
                crate::array::js_array_get_f64(array.get_raw_mut_ptr(), slot as u32).to_bits(),
            )
            .as_pointer::<crate::object::ObjectHeader>();
            let id = shapes::object_shape_stamp(object);
            let d = shapes::shape_descriptor_by_id(id).unwrap();
            assert_eq!(d.rep, REP, "F64 and CF must both survive reconstruction");
            assert_eq!(d.constfn_infos()[0].info, info() as usize as u64);
            let fields = (object as *const u8)
                .add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *const u64;
            let c = JSValue::from_bits(*fields).as_pointer::<ClosureHeader>();
            assert_eq!(capture_body(c, closure::JsThis::UNDEFINED), expected);
            assert_eq!(f64::from_bits(*fields.add(1)), expected);
            ids.push(id);
            cells.push(c as usize);
        }
        assert_eq!(
            ids[0], ids[1],
            "same static body shares facts across fresh closures"
        );
        assert_ne!(
            cells[0], cells[1],
            "rebuilt closures must keep distinct captures"
        );
        if seed_first || production_seed {
            assert_eq!(ids[0], FINAL);
        } else {
            assert!(!shapes::is_static_shape_id(ids[0]));
            assert!(shapes::shape_descriptor_by_id(FINAL).is_none());
            // Object-first must keep its old carriers valid when the external
            // static id arrives, and subsequent reconstructions use that id.
            shapes::install_worker_shape_seed(&seed);
            let bits = deserialize_nanbox_on_current_thread(&payload);
            let array = JSValue::from_bits(bits).as_pointer::<crate::array::ArrayHeader>();
            let obj = JSValue::from_bits(crate::array::js_array_get_f64(array, 0).to_bits())
                .as_pointer::<crate::object::ObjectHeader>();
            assert_eq!(shapes::object_shape_stamp(obj), FINAL);
            assert!(shapes::shape_descriptor_by_id(ids[0]).is_some());
        }
    })
    .join()
    .expect("receiver agent");
}
#[test]
fn constfn_transfer_seed_first_keeps_bodies_captures_and_numeric_rep() {
    let _lock = gc::global_side_table_test_lock();
    replay(true, false);
}
#[test]
fn constfn_transfer_object_first_mints_locally_then_installs_external_facts() {
    let _lock = gc::global_side_table_test_lock();
    replay(false, false);
}
#[test]
fn constfn_transfer_production_worker_seed_preserves_body_metadata() {
    let _lock = gc::global_side_table_test_lock();
    replay(false, true);
}

#[test]
fn constfn_transfer_refuses_rebuilt_numeric_contradiction() {
    let _lock = gc::global_side_table_test_lock();
    let mut payload = unsafe { wire() };
    let SerializedValue::Array(objects) = &mut payload else {
        unreachable!()
    };
    let SerializedValue::Object { fields, .. } = &mut objects[0] else {
        unreachable!()
    };
    fields[1] = SerializedValue::Inline(TAG_TRUE);
    std::thread::spawn(move || unsafe {
        seed_final();
        let bits = deserialize_nanbox_on_current_thread(&payload);
        let array = JSValue::from_bits(bits).as_pointer::<crate::array::ArrayHeader>();
        let object = JSValue::from_bits(crate::array::js_array_get_f64(array, 0).to_bits())
            .as_pointer::<crate::object::ObjectHeader>();
        let d = shapes::object_shape_descriptor(object).unwrap();
        assert_ne!(shapes::object_shape_stamp(object), FINAL);
        assert_eq!(
            d.special_constfn_mask, 0,
            "a wire fact cannot override a current field value"
        );
    })
    .join()
    .expect("receiver agent");
}

#[test]
fn constfn_transfer_final_slots_rewrite_the_actual_closures_on_moving_gc() {
    let _guard = gc::CopyingNurseryTestGuard::new(0);
    let _triggers = gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _forced = gc::knob_overrides::ForcedEvacuationTestGuard::on();
    gc::register_runtime_handle_root_scanner_for_tests();
    gc::gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc::gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc::gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc::gc_register_mutable_root_scanner(shapes::scan_shape_table_rekey_mut);
    let previous =
        gc::set_conservative_stack_scan_override(Some(gc::ConservativeStackScanMode::Disabled));
    struct Restore(Option<gc::ConservativeStackScanMode>);
    impl Drop for Restore {
        fn drop(&mut self) {
            gc::set_conservative_stack_scan_override(self.0);
        }
    }
    let _restore = Restore(previous);
    let scope = gc::RuntimeHandleScope::new();
    unsafe {
        let payload = wire();
        let bits = deserialize_nanbox_on_current_thread(&payload);
        let array = scope.root_raw_mut_ptr(
            JSValue::from_bits(bits).as_pointer::<crate::array::ArrayHeader>()
                as *mut crate::array::ArrayHeader,
        );
        let snapshot = || {
            (0..2)
                .map(|i| {
                    let obj = JSValue::from_bits(
                        crate::array::js_array_get_f64(array.get_raw_mut_ptr(), i).to_bits(),
                    )
                    .as_pointer::<crate::object::ObjectHeader>();
                    let method = crate::object::js_object_get_field(obj as *mut _, 0).bits();
                    (obj as usize, (method & POINTER_MASK) as usize)
                })
                .collect::<Vec<_>>()
        };
        let before = snapshot();
        assert!(
            before
                .iter()
                .all(|(o, c)| crate::arena::pointer_in_nursery(*o)
                    && crate::arena::pointer_in_nursery(*c)),
            "premise: exact receiver and closure cells are movable"
        );
        gc::gc_collect_minor();
        let after = snapshot();
        for (i, (old, new)) in before.iter().zip(&after).enumerate() {
            assert_ne!(old.0, new.0, "actual receiver {i} did not relocate");
            assert_ne!(old.1, new.1, "current closure slot {i} did not rewrite");
            let obj = new.0 as *const crate::object::ObjectHeader;
            assert_eq!(shapes::object_shape_stamp(obj), FINAL);
            assert_eq!(
                capture_body(new.1 as *const ClosureHeader, closure::JsThis::UNDEFINED),
                [17.0, 29.0][i]
            );
        }
    }
}

#[test]
fn constfn_transfer_does_not_resurrect_a_deprecated_final_body_fact() {
    let _lock = gc::global_side_table_test_lock();
    std::thread::spawn(move || unsafe {
        let payload = wire();
        let scope = gc::RuntimeHandleScope::new();
        let bits = deserialize_nanbox_on_current_thread(&payload);
        let arr = scope.root_raw_mut_ptr(
            JSValue::from_bits(bits).as_pointer::<crate::array::ArrayHeader>()
                as *mut crate::array::ArrayHeader,
        );
        let obj =
            JSValue::from_bits(crate::array::js_array_get_f64(arr.get_raw_mut_ptr(), 0).to_bits())
                .as_pointer::<crate::object::ObjectHeader>()
                as *mut crate::object::ObjectHeader;
        assert_eq!(shapes::object_shape_stamp(obj), FINAL);
        crate::object::store_object_field_slot(obj, 0, TAG_TRUE);
        assert_eq!(
            shapes::shape_descriptor_by_id(FINAL)
                .unwrap()
                .deprecation_targets()
                .1,
            1
        );
        let bits = deserialize_nanbox_on_current_thread(&payload);
        let arr = JSValue::from_bits(bits).as_pointer::<crate::array::ArrayHeader>();
        let obj = JSValue::from_bits(crate::array::js_array_get_f64(arr, 0).to_bits())
            .as_pointer::<crate::object::ObjectHeader>();
        assert_ne!(
            shapes::object_shape_stamp(obj),
            FINAL,
            "a learned final record cannot be republished on a new receiver"
        );
        assert_eq!(
            shapes::object_shape_descriptor(obj)
                .unwrap()
                .special_constfn_mask,
            0
        );
    })
    .join()
    .expect("isolated deprecation agent");
}

/// The seed outlives a copying collection in the source agent. Startup plain
/// seeds use immortal canonical keys, so this fixture deliberately passes
/// nursery keys through the real external class-final mint instead. It tests
/// the worker seed's ownership contract, not movement of immortal startup keys.
#[test]
fn constfn_worker_seed_owns_names_across_source_key_relocation() {
    const LONG: &[u8] = b"cfseed_lifetime_method";
    const SHORT: &[u8] = b"n";
    const NAMES: &[u8] = b"cfseed_lifetime_method\0n\0";
    const CLASS: u32 = 0x7991;
    const BASE_ID: u32 = shapes::SHAPE_ID_BASE + 0x7973;
    const FINAL_ID: u32 = shapes::SHAPE_ID_BASE + 0x7974;
    const NUMBER_REP: u64 = field_rep::REP_F64 << 2;

    unsafe fn names_at(keys: u64) -> Vec<Vec<u8>> {
        let (slots, len) = crate::object::keys_array_dense_slots(
            keys as usize as *const crate::array::ArrayHeader,
        );
        assert!(!slots.is_null());
        assert_eq!(len, 2);
        (0..2)
            .map(|slot| {
                let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
                crate::string::js_string_key_bytes(
                    JSValue::from_bits((*slots.add(slot)).to_bits()),
                    &mut scratch,
                )
                .expect("each source/worker key must decode")
                .to_vec()
            })
            .collect()
    }
    unsafe fn heap_key_at(keys: u64) -> usize {
        let (slots, len) = crate::object::keys_array_dense_slots(
            keys as usize as *const crate::array::ArrayHeader,
        );
        assert!(!slots.is_null());
        assert_eq!(len, 2);
        let bits = (*slots).to_bits();
        assert_eq!(bits & TAG_MASK, STRING_TAG, "long key must be heap encoded");
        assert!(JSValue::from_bits((*slots.add(1)).to_bits()).is_short_string());
        (bits & POINTER_MASK) as usize
    }

    let _guard = gc::CopyingNurseryTestGuard::new(0);
    let _triggers = gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _forced = gc::knob_overrides::ForcedEvacuationTestGuard::on();
    gc::register_runtime_handle_root_scanner_for_tests();
    gc::gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc::gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc::gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc::gc_register_mutable_root_scanner(shapes::scan_shape_table_rekey_mut);
    let previous =
        gc::set_conservative_stack_scan_override(Some(gc::ConservativeStackScanMode::Disabled));
    struct Restore(Option<gc::ConservativeStackScanMode>);
    impl Drop for Restore {
        fn drop(&mut self) {
            gc::set_conservative_stack_scan_override(self.0);
        }
    }
    let _restore = Restore(previous);
    let scope = gc::RuntimeHandleScope::new();
    unsafe {
        let keys = scope.root_raw_mut_ptr(crate::array::js_array_alloc_key_list(2, true));
        let long = crate::string::js_string_from_bytes(LONG.as_ptr(), LONG.len() as u32);
        store_thread_array_slot(keys.get_raw_mut_ptr(), 0, JSValue::string_ptr(long).bits());
        store_thread_array_slot(
            keys.get_raw_mut_ptr(),
            1,
            JSValue::try_short_string(SHORT)
                .expect("short-key premise")
                .bits(),
        );
        assert_eq!(
            static_shapes::js_object_shape_id_for_class_keys_static(
                keys.get_raw_mut_ptr::<crate::array::ArrayHeader>() as usize as u64,
                2,
                2,
                CLASS,
                BASE_ID,
                NUMBER_REP,
            ),
            BASE_ID
        );
        let object =
            scope.root_raw_mut_ptr(crate::object::js_object_alloc_class_inline_keys_stamped(
                CLASS,
                0,
                2,
                keys.get_raw_mut_ptr(),
                BASE_ID,
                NUMBER_REP,
            ));
        let closure = scope.root_raw_mut_ptr(closure::js_closure_alloc(info(), 1));
        closure::js_closure_set_capture_bits(closure.get_raw_mut_ptr(), 0, 37.0f64.to_bits());
        crate::object::store_object_field_slot(
            object.get_raw_mut_ptr(),
            0,
            JSValue::object_ptr(closure.get_raw_mut_ptr::<u8>()).bits(),
        );
        crate::object::store_object_field_slot(object.get_raw_mut_ptr(), 1, 29.0f64.to_bits());
        let entries = [static_shapes::ConstFnStaticEntry {
            slot: 0,
            info: info(),
        }];
        assert_eq!(
            static_shapes::js_object_final_shape_id_for_class_keys_static_constfn(
                keys.get_raw_mut_ptr::<crate::array::ArrayHeader>() as usize as u64,
                2,
                2,
                CLASS,
                FINAL_ID,
                REP,
                entries.as_ptr(),
                1,
            ),
            FINAL_ID,
            "production mint must publish the real external final carrier"
        );
        let finalized = static_shapes::js_object_finalize_constfn_static(
            object.get_raw_mut_ptr::<crate::object::ObjectHeader>() as usize as u64,
            FINAL_ID,
            NAMES.as_ptr(),
            NAMES.len() as u32,
            2,
            2,
            CLASS,
            REP,
            entries.as_ptr(),
            1,
        ) as usize as *const crate::object::ObjectHeader;
        assert_eq!(shapes::object_shape_stamp(finalized), FINAL_ID);
        let before = shapes::shape_descriptor_by_id(FINAL_ID).unwrap();
        let before_heap_key = heap_key_at(before.keys);
        assert!(
            crate::arena::pointer_in_nursery(before.keys as usize),
            "premise: external record's source key list must be nursery movable"
        );
        assert!(
            crate::arena::pointer_in_nursery(before_heap_key),
            "premise: the long key bytes must be nursery movable"
        );
        assert_eq!(names_at(before.keys), vec![LONG.to_vec(), SHORT.to_vec()]);
        assert_eq!(before.rep, REP);
        assert_eq!(
            before.constfn_infos(),
            &[shapes::ConstFnSlotInfo {
                slot: 0,
                info: info() as usize as u64
            }]
        );

        let seed = shapes::worker_shape_seed();
        let payload = serialize_nanbox_for_thread(JSValue::pointer(finalized.cast()).bits());
        assert!(matches!(
            &payload,
            SerializedValue::Object {
                final_constfn: Some(_),
                ..
            }
        ));
        // This is the lifetime window: the Rust-owned seed already exists, but
        // no receiving worker has installed it. Neither saved old address is
        // dereferenced after collection.
        gc::gc_collect_minor();
        let after = shapes::shape_descriptor_by_id(FINAL_ID).unwrap();
        assert_ne!(
            after.keys, before.keys,
            "source key list did not actually relocate"
        );
        assert_ne!(
            heap_key_at(after.keys),
            before_heap_key,
            "heap key bytes did not actually relocate"
        );
        assert_eq!(
            after.keys,
            keys.get_raw_mut_ptr::<crate::array::ArrayHeader>() as usize as u64
        );
        assert_eq!(names_at(after.keys), vec![LONG.to_vec(), SHORT.to_vec()]);
        assert_eq!(
            shapes::final_shape_ensure_constfn(
                after.keys as usize as *const crate::array::ArrayHeader,
                2,
                2,
                CLASS,
                REP,
                after.constfn_infos(),
                None,
            )
            .unwrap(),
            FINAL_ID,
            "source facts accelerator must be rekeyed, not mint a second descriptor"
        );
        let relocated_source_keys = after.keys;
        std::thread::spawn(move || {
            let _agent = crate::agent::enter_worker_agent();
            gc::ensure_gc_initialized();
            assert!(
                shapes::shape_descriptor_by_id(FINAL_ID).is_none(),
                "fresh worker premise"
            );
            shapes::install_worker_shape_seed(&seed);
            let installed = shapes::shape_descriptor_by_id(FINAL_ID)
                .expect("saved seed must install before object reconstruction can mask a failure");
            assert_ne!(
                installed.keys, relocated_source_keys,
                "worker must own its key storage"
            );
            assert_eq!(
                names_at(installed.keys),
                vec![LONG.to_vec(), SHORT.to_vec()]
            );
            assert_eq!(installed.logical_key_count, 2);
            assert_eq!(installed.live_inline_slot_count, 2);
            assert_eq!(installed.proto_id, shapes::class_proto_id(CLASS));
            assert_eq!(installed.rep, REP);
            assert_eq!(installed.special_constfn_mask, 1);
            assert_eq!(installed.deprecation_targets(), (0, 0));
            assert_eq!(
                installed.constfn_infos(),
                &[shapes::ConstFnSlotInfo {
                    slot: 0,
                    info: info() as usize as u64
                }]
            );
            let bits = deserialize_nanbox_on_current_thread(&payload);
            let object = JSValue::from_bits(bits).as_pointer::<crate::object::ObjectHeader>();
            assert_eq!(shapes::object_shape_stamp(object), FINAL_ID);
            let fields = (object as *const u8)
                .add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *const u64;
            let current_closure = JSValue::from_bits(*fields).as_pointer::<ClosureHeader>();
            assert_eq!(
                capture_body(current_closure, closure::JsThis::UNDEFINED),
                37.0
            );
            assert_eq!(*fields.add(1), 29.0f64.to_bits());
        })
        .join()
        .expect("receiving agent after source-key movement");
    }
}

/// Replays worker-producer.ts's compiler wire: class header2 is a registered
/// closed-literal shape, x is F64 slot0, m is ConstFn slot1, and the finalizer
/// is passed plain class0. Sharing the image must precede shape-seed install.
#[test]
fn constfn_transfer_compiled_anon_header_preserves_actual_worker_wire() {
    const ANON: u32 = 2;
    const ANON_BASE: u32 = shapes::SHAPE_ID_BASE + 0x3c;
    const ANON_FINAL: u32 = shapes::SHAPE_ID_BASE + 0x46;
    const NAMES: &[u8] = b"x\0m\0";
    const ANON_REP: u64 = field_rep::REP_F64 | (field_rep::REP_SPECIAL << 2);
    fn arrow_info() -> *const closure::JsFunctionInfo {
        crate::fn_info!(capture_body, 0; with_declared(0), with_length(0),
            with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE
                | closure::FN_ARROW | closure::FN_STRICT))
    }
    let _lock = gc::global_side_table_test_lock();
    std::thread::spawn(move || unsafe {
        crate::object::class_image::enter_current_thread_image();
        crate::object::js_register_anon_shape_class_id(ANON);
        assert!(crate::object::is_anon_shape_class_id(ANON));
        assert_eq!(shapes::class_proto_id(ANON), shapes::class_proto_id(0));
        assert_eq!((*arrow_info()).flags, 6200, "actual producer info flags");
        assert_eq!(
            static_shapes::js_shape_seed_plain(
                ANON_BASE,
                NAMES.as_ptr(),
                NAMES.len() as u32,
                2,
                2,
                field_rep::REP_F64,
            ),
            ANON_BASE,
        );
        let entries = [static_shapes::ConstFnStaticEntry {
            slot: 1,
            info: arrow_info(),
        }];
        assert_eq!(
            static_shapes::js_shape_seed_plain_constfn(
                ANON_FINAL,
                NAMES.as_ptr(),
                NAMES.len() as u32,
                2,
                2,
                ANON_REP,
                entries.as_ptr(),
                1,
            ),
            ANON_FINAL,
        );
        let scope = gc::RuntimeHandleScope::new();
        let keys = crate::object::js_build_class_keys_array(
            ANON,
            2,
            NAMES.as_ptr(),
            NAMES.len() as u32,
            field_rep::REP_F64,
        ) as usize as *mut crate::array::ArrayHeader;
        let keys = scope.root_raw_mut_ptr(keys);
        let mut objects = Vec::new();
        for number in [17.0f64, 29.0] {
            let object =
                scope.root_raw_mut_ptr(crate::object::js_object_alloc_class_inline_keys_stamped(
                    ANON,
                    0,
                    2,
                    keys.get_raw_mut_ptr(),
                    ANON_BASE,
                    field_rep::REP_F64,
                ));
            let cell = scope.root_raw_mut_ptr(closure::js_closure_alloc(arrow_info(), 1));
            closure::js_closure_set_capture_bits(cell.get_raw_mut_ptr(), 0, number.to_bits());
            crate::object::store_object_field_slot(object.get_raw_mut_ptr(), 0, number.to_bits());
            crate::object::store_object_field_slot(
                object.get_raw_mut_ptr(),
                1,
                JSValue::pointer(cell.get_raw_mut_ptr::<u8>()).bits(),
            );
            let object = static_shapes::js_object_finalize_constfn_static(
                object.get_raw_mut_ptr::<crate::object::ObjectHeader>() as usize as u64,
                ANON_FINAL,
                NAMES.as_ptr(),
                NAMES.len() as u32,
                2,
                2,
                0,
                ANON_REP,
                entries.as_ptr(),
                1,
            ) as usize as *mut crate::object::ObjectHeader;
            assert_eq!(
                (*object).class_id,
                ANON,
                "real producer header must survive"
            );
            assert_eq!(shapes::object_shape_stamp(object), ANON_FINAL);
            let payload = serialize_nanbox_for_thread(JSValue::pointer(object.cast()).bits());
            assert!(
                matches!(&payload, SerializedValue::Object {
                class_id: ANON, parent_class_id: 0, final_constfn: Some(_), keys: Some(names), ..
            } if names == &vec![b"x".to_vec(), b"m".to_vec()]),
                "actual anonymous producer must publish final wire facts"
            );
            objects.push(payload);
        }
        let image = crate::object::class_image::current_image_handle();
        let seed = shapes::worker_shape_seed();
        std::thread::spawn(move || {
            crate::object::class_image::adopt_image(image);
            let agent = crate::agent::enter_worker_agent();
            gc::ensure_gc_initialized();
            shapes::install_worker_shape_seed(&seed);
            assert_eq!(
                shapes::shape_descriptor_by_id(ANON_FINAL).unwrap().rep,
                ANON_REP
            );
            assert_eq!(shapes::class_proto_id(ANON), shapes::class_proto_id(0));
            let handles = gc::RuntimeHandleScope::new();
            let mut cells = Vec::new();
            for (payload, expected) in objects.iter().zip([17.0, 29.0]) {
                let bits = deserialize_nanbox_on_current_thread(payload);
                let object = handles.root_raw_mut_ptr(
                    JSValue::from_bits(bits).as_pointer::<crate::object::ObjectHeader>()
                        as *mut crate::object::ObjectHeader,
                );
                let object = object.get_raw_mut_ptr::<crate::object::ObjectHeader>();
                assert_eq!((*object).class_id, ANON);
                assert_eq!(
                    shapes::object_shape_stamp(object),
                    ANON_FINAL,
                    "worker must republish actual final facts after validating current slots"
                );
                let d = shapes::object_shape_descriptor(object).unwrap();
                assert_eq!(d.rep, ANON_REP);
                assert_eq!(d.special_constfn_mask, 2);
                assert_eq!(
                    d.constfn_infos(),
                    &[shapes::ConstFnSlotInfo {
                        slot: 1,
                        info: arrow_info() as usize as u64,
                    }]
                );
                let fields = (object as *const u8)
                    .add(std::mem::size_of::<crate::object::ObjectHeader>())
                    as *const u64;
                assert_eq!(f64::from_bits(*fields), expected);
                let cell = JSValue::from_bits(*fields.add(1)).as_pointer::<ClosureHeader>();
                assert_eq!(capture_body(cell, closure::JsThis::UNDEFINED), expected);
                cells.push(cell as usize);
            }
            assert_ne!(cells[0], cells[1], "same body keeps distinct captures");
            let mut contradiction = objects.remove(0);
            let SerializedValue::Object { fields, .. } = &mut contradiction else {
                unreachable!()
            };
            fields[0] = SerializedValue::Inline(TAG_TRUE);
            let bits = deserialize_nanbox_on_current_thread(&contradiction);
            let object = JSValue::from_bits(bits).as_pointer::<crate::object::ObjectHeader>();
            assert_eq!(
                shapes::object_shape_descriptor(object)
                    .unwrap()
                    .special_constfn_mask,
                0,
                "wire cannot override a contradictory current numeric lane"
            );
            drop(handles);
            crate::agent::retire_agent(agent);
        })
        .join()
        .expect("actual anonymous receiving worker");
    })
    .join()
    .expect("isolated compiled producer image");
}
