use super::*;
use crate::gc::RuntimeHandle;

fn atom(text: &[u8]) -> *mut crate::StringHeader {
    let hash = crate::object::key_bytes_hash(text.as_ptr(), text.len());
    crate::string::js_string_pool_atom(text.as_ptr(), text.len() as u32, hash, 0)
}

/// An object literal's class: compiled literals carry an anonymous shape
/// class id, never 0 (class 0 is the read stub's refused band).
const DKR_ANON_CLASS_ID: u32 = 0x1075_3001;

fn literal_object() -> *mut ObjectHeader {
    unsafe { crate::object::js_register_anon_shape_class_id(DKR_ANON_CLASS_ID) };
    crate::object::js_object_alloc(DKR_ANON_CLASS_ID, 8)
}

fn boxed_obj(o: *const ObjectHeader) -> f64 {
    f64::from_bits(crate::value::js_nanbox_pointer(o as i64).to_bits())
}

fn boxed_key(k: *const crate::StringHeader) -> f64 {
    f64::from_bits(crate::value::js_nanbox_string(k as i64).to_bits())
}

/// `shape_answer` for a rooted receiver and key.
fn answer(obj: &RuntimeHandle<'_>, key: &RuntimeHandle<'_>) -> Option<u64> {
    obj.with_mut_ptr(|o: *mut ObjectHeader| {
        key.with_const_ptr(|k: *const crate::StringHeader| unsafe {
            shape_answer(boxed_obj(o).to_bits(), boxed_key(k).to_bits()).map(f64::to_bits)
        })
    })
}

/// The full entry, as the computed-read lowering calls it.
fn read(obj: &RuntimeHandle<'_>, key: &RuntimeHandle<'_>) -> u64 {
    obj.with_mut_ptr(|o: *mut ObjectHeader| {
        key.with_const_ptr(|k: *const crate::StringHeader| {
            js_typed_feedback_object_get_field_by_key_f64(0, o, boxed_key(k), boxed_obj(o))
                .to_bits()
        })
    })
}

fn set(obj: &RuntimeHandle<'_>, key: &RuntimeHandle<'_>, v: f64) {
    obj.with_mut_ptr(|o: *mut ObjectHeader| {
        key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, v))
    });
}

/// A pooled key is its text's atom, and so is the word the receiver's
/// canonical key list holds: the shape answers with one word compare per
/// position. A different string of the same text is not the list's word, so
/// the shape declines and the generic read answers the same value.
#[test]
fn a_present_key_is_answered_by_the_receivers_shape() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let a = scope.root_string_ptr(atom(b"dkr_present_a"));
    let b = scope.root_string_ptr(atom(b"dkr_present_b"));
    let obj = scope.root_raw_mut_ptr(literal_object());
    set(&obj, &a, 11.0);
    set(&obj, &b, 22.0);
    assert_eq!(answer(&obj, &a), Some(11.0f64.to_bits()));
    assert_eq!(answer(&obj, &b), Some(22.0f64.to_bits()));
    let copy = scope.root_string_ptr(crate::string::js_string_from_bytes(
        b"dkr_present_b".as_ptr(),
        13,
    ));
    assert_eq!(answer(&obj, &copy), None, "a non-atom key proves nothing");
    assert_eq!(read(&obj, &copy), 22.0f64.to_bits());
    assert_eq!(read(&obj, &b), 22.0f64.to_bits());
}

/// A key the shape's word compare cannot match (a non-atom string, or a key
/// past the inline positions) is answered by the receiver's own lookup, which
/// files the slot in the read stub: the next read of that text on a receiver
/// of this shape is the stub's, for an inline and an overflow slot alike. A
/// write to the key keeps the shape, and the stub reads the new value.
#[test]
fn an_own_key_the_words_miss_is_read_once_and_then_answered_by_the_stub() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _global = crate::object::js_get_global_this();
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(literal_object());
    // Twelve keys on an 8-slot literal: the last ones live in overflow slots.
    // Keys short enough to carry content bits (the stub key), and distinct
    // from every other test's.
    let names: Vec<Vec<u8>> = (0..12).map(|i| format!("Zo{i}").into_bytes()).collect();
    for (i, name) in names.iter().enumerate() {
        let key = scope.root_string_ptr(atom(name));
        set(&obj, &key, i as f64 + 0.5);
    }
    for i in [1usize, 11] {
        let copy = scope.root_string_ptr(crate::string::js_string_from_bytes(
            names[i].as_ptr(),
            names[i].len() as u32,
        ));
        assert_eq!(
            answer(&obj, &copy),
            None,
            "premise: nothing filed for key {i} yet"
        );
        assert_eq!(read(&obj, &copy), (i as f64 + 0.5).to_bits());
        assert_eq!(
            answer(&obj, &copy),
            Some((i as f64 + 0.5).to_bits()),
            "the own read must file key {i} in the stub"
        );
        let key = scope.root_string_ptr(atom(&names[i]));
        set(&obj, &key, 99.0);
        assert_eq!(read(&obj, &copy), 99.0f64.to_bits());
    }
}

/// An absent key is filed only after the generic read answered `undefined`,
/// and the verdict rests on `%Object.prototype%`'s ShapeId: adding the key
/// there drops it, and the read then finds the inherited value.
#[test]
fn an_absent_key_is_filed_after_the_get_and_dropped_when_object_prototype_changes() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _global = crate::object::js_get_global_this();
    let scope = crate::gc::RuntimeHandleScope::new();
    let a = scope.root_string_ptr(atom(b"dkr_abs_a"));
    let missing = scope.root_string_ptr(atom(b"Zq7"));
    let obj = scope.root_raw_mut_ptr(literal_object());
    set(&obj, &a, 1.0);
    assert_eq!(answer(&obj, &missing), None, "premise: nothing filed yet");
    assert_eq!(read(&obj, &missing), crate::value::TAG_UNDEFINED);
    assert_eq!(
        answer(&obj, &missing),
        Some(crate::value::TAG_UNDEFINED),
        "the confirmed absence is answered from shapes"
    );
    let proto = crate::array::object_prototype_addr();
    assert_ne!(proto, 0, "premise: Object.prototype exists");
    let proto = scope.root_raw_mut_ptr(proto as *mut ObjectHeader);
    set(&proto, &missing, 5.0);
    assert_eq!(
        answer(&obj, &missing),
        None,
        "Object.prototype's new ShapeId must retire the verdict"
    );
    assert_eq!(read(&obj, &missing), 5.0f64.to_bits());
}

/// A receiver whose shape links to null needs no terminal: the receiver's own
/// ShapeId is the whole proof. `setPrototypeOf(o, null)` records the null link
/// in the shape.
#[test]
fn a_null_prototype_receiver_files_its_absence_without_a_terminal() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _global = crate::object::js_get_global_this();
    let scope = crate::gc::RuntimeHandleScope::new();
    let a = scope.root_string_ptr(atom(b"dkr_null_a"));
    let missing = scope.root_string_ptr(atom(b"Zq8"));
    let obj = scope.root_raw_mut_ptr(literal_object());
    set(&obj, &a, 3.0);
    obj.with_mut_ptr(|o: *mut ObjectHeader| {
        crate::object::js_object_set_prototype_of(
            boxed_obj(o),
            f64::from_bits(crate::value::TAG_NULL),
        )
    });
    let terminal = obj.with_mut_ptr(|o: *mut ObjectHeader| unsafe {
        crate::object::method_site::read_holder::dynamic_absent_terminal(o, b"Zq8")
    });
    assert_eq!(terminal, Some(0));
    assert_eq!(read(&obj, &missing), crate::value::TAG_UNDEFINED);
    assert_eq!(answer(&obj, &missing), Some(crate::value::TAG_UNDEFINED));
    assert_eq!(read(&obj, &a), 3.0f64.to_bits());
}

/// A born-null receiver publishes its null edge in the birth shape. The
/// shape proves absence, while the read stub still refuses class-zero owners.
#[test]
fn a_born_null_receiver_records_its_edge_while_the_stub_refuses_class_zero() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _global = crate::object::js_get_global_this();
    let scope = crate::gc::RuntimeHandleScope::new();
    let a = scope.root_string_ptr(atom(b"dkr_cnull_a"));
    let missing = scope.root_string_ptr(atom(b"Zqb"));
    let created = crate::object::js_object_create(f64::from_bits(crate::value::TAG_NULL));
    let obj = scope.root_raw_mut_ptr(
        (created.to_bits() & crate::value::POINTER_MASK) as usize as *mut ObjectHeader,
    );
    set(&obj, &a, 3.0);
    let (recorded, actual) = obj.with_mut_ptr(|o: *mut ObjectHeader| unsafe {
        let stamp = crate::object::shapes::object_shape_stamp(o);
        (
            crate::object::shapes::shape_proto_id(stamp),
            crate::object::shapes::object_proto_id(o),
        )
    });
    assert_eq!(actual, crate::object::shapes::PROTO_ID_NULL);
    assert_eq!(recorded, Some(actual), "the birth shape owns the null link");
    let terminal = obj.with_mut_ptr(|o: *mut ObjectHeader| unsafe {
        crate::object::method_site::read_holder::dynamic_absent_terminal(o, b"Zqb")
    });
    assert_eq!(
        terminal,
        Some(0),
        "the receiver needs no prototype terminal"
    );
    assert_eq!(read(&obj, &missing), crate::value::TAG_UNDEFINED);
    assert_eq!(answer(&obj, &missing), None, "class zero is not cacheable");
    assert_eq!(read(&obj, &a), 3.0f64.to_bits());
}

/// What the shapes must never call absent: an own key whose VALUE is
/// `undefined` (the Get answers `undefined` too, but the key is there), and an
/// index-like name, which elements can hold outside the key list.
#[test]
fn an_own_undefined_value_or_an_index_name_is_never_absent() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _global = crate::object::js_get_global_this();
    let scope = crate::gc::RuntimeHandleScope::new();
    let u = scope.root_string_ptr(atom(b"Zq9"));
    let obj = scope.root_raw_mut_ptr(literal_object());
    set(&obj, &u, f64::from_bits(crate::value::TAG_UNDEFINED));
    let own = obj.with_mut_ptr(|o: *mut ObjectHeader| unsafe {
        crate::object::method_site::read_holder::dynamic_absent_terminal(o, b"Zq9")
    });
    assert_eq!(own, None, "an own key is not absent, whatever its value");
    let index = obj.with_mut_ptr(|o: *mut ObjectHeader| unsafe {
        crate::object::method_site::read_holder::dynamic_absent_terminal(o, b"0")
    });
    assert_eq!(
        index, None,
        "an index-like name is not the shapes' to answer"
    );
    // Positive control: a name the receiver really lacks.
    let lacks = obj.with_mut_ptr(|o: *mut ObjectHeader| unsafe {
        crate::object::method_site::read_holder::dynamic_absent_terminal(o, b"Zqa")
    });
    assert!(lacks.is_some(), "the control must be provable absent");
}
