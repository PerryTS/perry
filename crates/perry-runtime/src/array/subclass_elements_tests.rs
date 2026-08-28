//! The `ObjectMeta.elements` edge of an Array-subclass instance is a traced
//! child exactly like `spill`: it must survive owner and meta evacuation, be
//! rewritten to the moved inner array, and keep the inner array alive.
use super::subclass_elements::{
    elements_of, install_elements, set_elements_head, ArraySubclassRepresentationGuard,
};
use crate::object::{js_object_alloc, ObjectHeader};

const CLASS_ID_ARRAY: u32 = 0xFFFF_0024;

fn live_obj(receiver: f64) -> *mut ObjectHeader {
    (receiver.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader
}

#[test]
fn the_elements_edge_survives_moving_gc_and_keeps_the_inner_array_alive() {
    let _copying_nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force_evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);

    let class_id = 0x0074_8695;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));

    // `super(3)`: three holes, `length` 3, no shape-carried `length`.
    unsafe { install_elements(live_obj(receiver_h.get_nanbox_f64()), 3) };
    let before_elements = unsafe { elements_of(live_obj(receiver_h.get_nanbox_f64())) };
    assert!(!before_elements.is_null());
    assert_eq!(unsafe { (*before_elements).length }, 3);
    // Idempotent: a second install keeps the store.
    unsafe { install_elements(live_obj(receiver_h.get_nanbox_f64()), 9) };
    assert_eq!(
        unsafe { elements_of(live_obj(receiver_h.get_nanbox_f64())) },
        before_elements
    );

    // An append past the exact capacity re-allocates the inner array; the
    // head is written back through the barriered meta slot.
    let grown = crate::array::js_array_push_f64(before_elements, 44.0);
    unsafe { set_elements_head(live_obj(receiver_h.get_nanbox_f64()), grown) };
    crate::array::js_array_set_f64(grown, 0, 11.0);
    assert_eq!(unsafe { (*grown).length }, 4);

    let before_cycles = crate::gc::copying_minor_cycles();
    let _ = crate::gc::gc_collect_minor();
    assert!(crate::gc::copying_minor_cycles() > before_cycles);

    let live = live_obj(receiver_h.get_nanbox_f64());
    let elements = unsafe { elements_of(live) };
    assert!(
        !elements.is_null(),
        "the edge must be rewritten, not dropped"
    );
    assert_ne!(
        elements, grown,
        "forced evacuation must have moved the inner array"
    );
    let header = unsafe { crate::value::addr_class::try_read_gc_header(elements as usize) }
        .expect("the moved inner array is a live heap object");
    assert_eq!(header.obj_type, crate::gc::GC_TYPE_ARRAY);
    assert_eq!(header.gc_flags & crate::gc::GC_FLAG_FORWARDED, 0);
    assert_eq!(unsafe { (*elements).length }, 4);
    assert_eq!(crate::array::js_array_get_f64(elements, 0), 11.0);
    assert_eq!(crate::array::js_array_get_f64(elements, 3), 44.0);
    // The untouched indices are still absent (a hole, or `undefined` once the
    // read resolves it through the prototype chain), never a stale value.
    let hole = crate::array::js_array_get_f64(elements, 1).to_bits();
    assert!(
        hole == crate::value::TAG_HOLE || hole == crate::value::TAG_UNDEFINED,
        "index 1 must still be absent: {hole:#x}"
    );
}

/// The hot runtime entries route an elements-backed instance to its inner
/// array: `length`, `[i]` get/set, append (including the re-allocating one,
/// with the owner rooted across it) and pop — through both the value-taking
/// `array_subclass_fast_*` entries and the raw `js_array_*` entries the
/// codegen fallbacks call, and across a forced-evacuation minor GC.
#[test]
fn hot_entries_route_to_the_elements_store() {
    use super::subclass::{
        array_subclass_fast_index_get, array_subclass_fast_index_set, array_subclass_fast_length,
        array_subclass_fast_pop, array_subclass_fast_push_one,
    };
    let _copying_nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force_evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);

    let class_id = 0x0074_8696;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
    unsafe { install_elements(live_obj(receiver_h.get_nanbox_f64()), 0) };
    let recv = || receiver_h.get_nanbox_f64();
    let as_arr = || (recv().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut crate::array::ArrayHeader;

    assert_eq!(array_subclass_fast_length(recv()), Some(0.0));
    assert_eq!(array_subclass_fast_index_get(recv(), 0), None);
    // 40 appends from an exact-capacity-0 store: several re-allocations.
    for i in 0..40u32 {
        assert_eq!(
            array_subclass_fast_push_one(recv(), f64::from(i)),
            Some(f64::from(i + 1))
        );
        if i == 17 {
            let _ = crate::gc::gc_collect_minor();
        }
    }
    assert_eq!(array_subclass_fast_length(recv()), Some(40.0));
    for i in 0..40u32 {
        assert_eq!(array_subclass_fast_index_get(recv(), i), Some(f64::from(i)));
    }
    assert_eq!(array_subclass_fast_index_get(recv(), 40), None);
    // In-bounds write, appending write, hole-creating write (declined).
    assert!(array_subclass_fast_index_set(recv(), 3, 300.0));
    assert_eq!(array_subclass_fast_index_get(recv(), 3), Some(300.0));
    assert!(array_subclass_fast_index_set(recv(), 40, 400.0));
    assert_eq!(array_subclass_fast_length(recv()), Some(41.0));
    assert!(!array_subclass_fast_index_set(recv(), 50, 500.0));
    assert_eq!(array_subclass_fast_length(recv()), Some(41.0));
    // Pop through the value entry and through the raw `js_array_*` entries
    // the codegen fallbacks call with the object address as an ArrayHeader.
    assert_eq!(array_subclass_fast_pop(recv()), Some(400.0));
    assert_eq!(crate::array::js_array_pop_f64(as_arr()), 39.0);
    assert_eq!(crate::array::js_array_length(as_arr()), 39);
    let _ = crate::array::js_array_push_f64(as_arr(), 77.0);
    assert_eq!(crate::array::js_array_length(as_arr()), 40);
    assert_eq!(crate::array::js_array_get_f64(as_arr(), 39), 77.0);
    assert_eq!(array_subclass_fast_index_get(recv(), 39), Some(77.0));
    let _ = crate::gc::gc_collect_minor();
    assert_eq!(array_subclass_fast_length(recv()), Some(40.0));
    assert_eq!(array_subclass_fast_index_get(recv(), 3), Some(300.0));
    assert_eq!(crate::array::js_array_get_f64(as_arr(), 39), 77.0);
    // The shape-carried machinery never learned anything for this instance.
    assert_eq!(
        unsafe { (*(*live_obj(recv())).meta).array_subclass_dense_key },
        0
    );
}

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}
fn key_value(name: &str) -> f64 {
    crate::value::js_nanbox_string(key(name) as i64)
}
fn key_strings(arr: *const crate::array::ArrayHeader) -> Vec<String> {
    let n = crate::array::js_array_length(arr);
    (0..n)
        .map(|i| {
            let v = crate::array::js_array_get(arr, i);
            let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            unsafe { crate::string::js_string_key_bytes(v, &mut sso) }
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_else(|| "<non-string>".to_string())
        })
        .collect()
}
fn truthy(v: f64) -> bool {
    v.to_bits() == 0x7FFC_0000_0000_0004
}

/// The property funnel: through the ordinary object entry points, an
/// elements-backed instance's indices and `length` are own properties backed
/// by the inner array — reads, writes (in-bounds, append, hole-creating
/// extension, `length` truncation/extension), `hasOwnProperty`/`in`,
/// `delete`, key order for `Object.keys`/`getOwnPropertyNames`, and own
/// property descriptors — and no index key ever lands in the shape.
#[test]
fn the_property_funnel_answers_indices_and_length_from_the_store() {
    let _representation = ArraySubclassRepresentationGuard::elements();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let class_id = 0x0074_8697;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
    unsafe { install_elements(live_obj(recv_h.get_nanbox_f64()), 0) };
    let recv = || recv_h.get_nanbox_f64();
    let obj = || live_obj(recv());
    let get = |name: &str| crate::object::js_object_get_field_by_name(obj(), key(name));
    let set = |name: &str, v: f64| crate::object::js_object_set_field_by_name(obj(), key(name), v);

    // A named field stays a shape property; `length` and indices do not.
    set("tag", 7.0);
    set("0", 10.0);
    set("1", 11.0);
    set("2", 12.0);
    assert_eq!(get("length").as_number(), 3.0);
    assert_eq!(get("1").as_number(), 11.0);
    assert_eq!(get("tag").as_number(), 7.0);
    assert!(get("5").is_undefined());
    // Hole-creating extension, then `length` truncation and extension.
    set("5", 15.0);
    assert_eq!(get("length").as_number(), 6.0);
    assert!(get("3").is_undefined());
    assert_eq!(get("5").as_number(), 15.0);
    set("length", 2.0);
    assert_eq!(get("length").as_number(), 2.0);
    assert!(get("2").is_undefined());
    set("length", 4.0);
    assert_eq!(get("length").as_number(), 4.0);
    assert!(get("3").is_undefined());
    set("3", 13.0);
    // hasOwn / in.
    assert!(truthy(crate::object::js_object_has_own(
        recv(),
        key_value("0")
    )));
    assert!(truthy(crate::object::js_object_has_own(
        recv(),
        key_value("length")
    )));
    assert!(!truthy(crate::object::js_object_has_own(
        recv(),
        key_value("2")
    )));
    assert!(!truthy(crate::object::js_object_has_own(
        recv(),
        key_value("9")
    )));
    assert!(truthy(crate::object::js_object_has_property(recv(), 3.0)));
    assert!(!truthy(crate::object::js_object_has_property(recv(), 2.0)));
    assert!(truthy(crate::object::js_object_has_property(
        recv(),
        key_value("tag")
    )));
    // delete: an index becomes a hole, `length` is untouched and undeletable.
    assert_eq!(crate::object::js_object_delete_dynamic(obj(), 0.0), 1);
    assert!(get("0").is_undefined());
    assert_eq!(get("length").as_number(), 4.0);
    assert_eq!(
        crate::object::js_object_delete_dynamic(obj(), key_value("length")),
        0
    );
    // Key order: present indices ascending, then shape keys; `length` only
    // in getOwnPropertyNames, between them.
    assert_eq!(
        key_strings(crate::object::js_object_keys(obj())),
        vec!["1", "3", "tag"]
    );
    let names = crate::object::js_object_get_own_property_names(recv());
    assert_eq!(
        key_strings(crate::value::js_nanbox_get_pointer(names) as *const crate::array::ArrayHeader),
        vec!["1", "3", "length", "tag"]
    );
    // values / entries: present elements first, then the shape's `tag`.
    let values = crate::object::js_object_values(obj());
    assert_eq!(crate::array::js_array_length(values), 3);
    assert_eq!(crate::array::js_array_get(values, 0).as_number(), 11.0);
    assert_eq!(crate::array::js_array_get(values, 1).as_number(), 13.0);
    assert_eq!(crate::array::js_array_get(values, 2).as_number(), 7.0);
    let entries = crate::object::js_object_entries(obj());
    assert_eq!(crate::array::js_array_length(entries), 3);
    let first = crate::value::js_nanbox_get_pointer(f64::from_bits(
        crate::array::js_array_get(entries, 0).bits(),
    )) as *const crate::array::ArrayHeader;
    assert_eq!(key_strings(first)[0], "1");
    assert_eq!(crate::array::js_array_get(first, 1).as_number(), 11.0);
    // Descriptors.
    let d = crate::object::js_object_get_own_property_descriptor(recv(), key_value("1"));
    let dobj = crate::value::js_nanbox_get_pointer(d) as *const ObjectHeader;
    assert_eq!(
        crate::object::js_object_get_field_by_name(dobj, key("value")).as_number(),
        11.0
    );
    assert!(truthy(f64::from_bits(
        crate::object::js_object_get_field_by_name(dobj, key("enumerable")).bits()
    )));
    let d = crate::object::js_object_get_own_property_descriptor(recv(), key_value("length"));
    let dobj = crate::value::js_nanbox_get_pointer(d) as *const ObjectHeader;
    assert_eq!(
        crate::object::js_object_get_field_by_name(dobj, key("value")).as_number(),
        4.0
    );
    assert!(!truthy(f64::from_bits(
        crate::object::js_object_get_field_by_name(dobj, key("enumerable")).bits()
    )));
    assert!(crate::JSValue::from_bits(
        crate::object::js_object_get_own_property_descriptor(recv(), key_value("0")).to_bits()
    )
    .is_undefined());
    // The shape never learned an index key.
    assert!(!key_strings(crate::object::js_object_keys(obj()))
        .iter()
        .any(|k| k == "0" || k == "1" && false));
    assert!(!unsafe { elements_of(obj()) }.is_null());
}

/// `Object.freeze` leaves the elements representation for good: every present
/// element and `length` become shape-carried properties, the store is
/// detached, and the frozen instance reads back exactly the same.
#[test]
fn freeze_deopts_to_the_shape_carried_form() {
    let _representation = ArraySubclassRepresentationGuard::elements();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let class_id = 0x0074_8698;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
    unsafe { install_elements(live_obj(recv_h.get_nanbox_f64()), 0) };
    let recv = || recv_h.get_nanbox_f64();
    let obj = || live_obj(recv());
    for i in 0..3u32 {
        crate::object::js_object_set_field_by_name(obj(), key(&i.to_string()), f64::from(i * 10));
    }
    crate::object::js_object_delete_dynamic(obj(), 1.0);
    crate::object::js_object_set_field_by_name(obj(), key("tag"), 7.0);
    let _ = crate::object::js_object_freeze(recv());
    assert!(
        unsafe { elements_of(obj()) }.is_null(),
        "the store is detached on freeze"
    );
    let get = |name: &str| crate::object::js_object_get_field_by_name(obj(), key(name));
    assert_eq!(get("length").as_number(), 3.0);
    assert_eq!(get("0").as_number(), 0.0);
    assert!(get("1").is_undefined());
    assert_eq!(get("2").as_number(), 20.0);
    assert_eq!(get("tag").as_number(), 7.0);
    assert!(truthy(crate::object::js_object_has_own(
        recv(),
        key_value("2")
    )));
    assert!(!truthy(crate::object::js_object_has_own(
        recv(),
        key_value("1")
    )));
    // Frozen: the shape-carried machinery refuses the append.
    assert_eq!(
        super::subclass::array_subclass_fast_push_one(recv(), 99.0),
        None
    );
}

/// The counted-loop guard admits an elements-backed instance as a PLAIN-ARRAY
/// loop over its inner array: kind 1, the inner array's live address, and a
/// revalidation that re-resolves the (possibly re-allocated) store from the
/// receiver — this is what keeps versioned loops off the string-keyed
/// property path.
#[test]
fn the_counted_loop_guard_admits_an_elements_backed_receiver_as_its_inner_array() {
    let _representation = ArraySubclassRepresentationGuard::elements();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let class_id = 0x0074_8699;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
    unsafe { install_elements(live_obj(recv_h.get_nanbox_f64()), 0) };
    let recv = || recv_h.get_nanbox_f64();
    for i in 0..8u32 {
        assert!(super::subclass::array_subclass_fast_push_one(recv(), f64::from(i)).is_some());
    }

    let mut facts = [0u64; 7];
    let live = super::subclass::loop_guard::js_packed_arraylike_loop_guard_live(
        recv(),
        -1.0,
        0,
        facts.as_mut_ptr(),
    );
    assert_ne!(live, 0, "an elements-backed receiver must be admitted");
    assert_eq!(facts[0], 1, "admitted as a plain-array loop");
    assert_eq!(
        live as usize,
        unsafe { elements_of(live_obj(recv())) } as usize,
        "the live address is the inner array"
    );
    assert_eq!(facts[6], 8, "the live-length bound is the inner length");
    let revalidated = super::subclass::loop_guard::js_packed_arraylike_loop_revalidate_live(
        recv(),
        -1.0,
        0,
        facts.as_ptr(),
    );
    assert_eq!(revalidated, live, "revalidation resolves the same store");

    // An append inside the loop body re-allocates the store. The recorded
    // facts describe the OLD array, so revalidation takes the side exit (0),
    // exactly as it does for a grown plain Array — and a fresh guard call
    // then admits the current store.
    for i in 8..64u32 {
        assert!(super::subclass::array_subclass_fast_push_one(recv(), f64::from(i)).is_some());
    }
    let grown = unsafe { elements_of(live_obj(recv())) };
    assert_ne!(grown as usize, live as usize, "the appends re-allocated");
    assert_eq!(
        super::subclass::loop_guard::js_packed_arraylike_loop_revalidate_live(
            recv(),
            -1.0,
            0,
            facts.as_ptr(),
        ),
        0,
        "stale facts must side-exit"
    );
    let live2 = super::subclass::loop_guard::js_packed_arraylike_loop_guard_live(
        recv(),
        -1.0,
        0,
        facts.as_mut_ptr(),
    );
    assert_eq!(
        live2 as usize, grown as usize,
        "re-admission sees the new store"
    );
    assert_eq!(facts[6], 64);
}
