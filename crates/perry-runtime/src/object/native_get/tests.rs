use super::*;
use crate::object::{
    js_object_alloc, js_object_create, js_object_delete_field, js_object_freeze,
    js_object_set_field_by_name,
};
use std::sync::atomic::{AtomicUsize, Ordering};

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::canonical_key(name.as_bytes())
}

fn boxed(object: *const ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(object as i64)
}

fn object(name: &str, value: f64) -> *mut ObjectHeader {
    let object = js_object_alloc(0, 0);
    js_object_set_field_by_name(object, key(name), value);
    object
}

struct Slow(bool);
impl Slow {
    fn enter() -> Self {
        Self(FORCE_SLOW.with(|value| value.replace(true)))
    }
}
impl Drop for Slow {
    fn drop(&mut self) {
        FORCE_SLOW.with(|value| value.set(self.0));
    }
}

fn get(object: f64, name: &str) -> f64 {
    unsafe { get_by_canonical_key(object, key(name)) }
}

#[test]
fn own_data_slot_proof_comes_from_live_shape_and_retires_on_accessor() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let object = object("value", 7.0);
    let key = key("value");
    let slot = || unsafe {
        shapes::own_data_shape(shapes::ordinary_dir_addr(), (*object).shape_word())
            .flatten()
            .and_then(|shape| {
                shape.plain_slot(crate::value::nanbox_string_key(key).to_bits(), b"value")
            })
    };
    assert_eq!(slot().map(|(slot, _)| slot), Some(0));
    let byte_reads = crate::string::test_key_byte_reads();
    assert_eq!(
        unsafe { try_data_get_by_name(object, key) }.unwrap().bits(),
        7.0f64.to_bits()
    );
    assert_eq!(
        crate::string::test_key_byte_reads(),
        byte_reads,
        "the named read must preserve the key word through the shape lookup"
    );
    assert_eq!(
        unsafe { try_data_get_bytes(JSValue::from_bits(boxed(object).to_bits()), b"value") }
            .unwrap()
            .bits(),
        7.0f64.to_bits()
    );
    assert!(
        crate::string::test_key_byte_reads() > byte_reads,
        "the byte-only sibling must exercise the byte-read counter"
    );
    install_getter(object, "value", true);
    assert_eq!(
        slot(),
        None,
        "the successor shape must retire the plain-data proof"
    );
    differential(boxed(object), "value", 47.0);
}

fn differential(object: f64, name: &str, expected: f64) {
    assert_eq!(get(object, name).to_bits(), expected.to_bits());
    let _slow = Slow::enter();
    assert_eq!(get(object, name).to_bits(), expected.to_bits());
}

#[test]
fn own_data_is_served_and_matches_forced_slow_for_all_value_kinds() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let child = object("child", 1.0);
    let values = [
        7.0,
        -0.0,
        f64::from_bits(crate::value::TAG_UNDEFINED),
        f64::from_bits(crate::value::TAG_NULL),
        f64::from_bits(crate::value::TAG_TRUE),
        crate::value::js_nanbox_string(key("a heap string value") as i64),
        boxed(child),
    ];
    for value in values {
        let object = object("value", value);
        let fast = unsafe { try_data_get_by_name(object, key("value")) };
        assert_eq!(fast.map(|value| value.bits()), Some(value.to_bits()));
        differential(boxed(object), "value", value);
    }
}

#[test]
fn prototype_data_mutation_shadow_delete_and_freeze_match_forced_slow() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let prototype = object("value", 7.0);
    let middle = js_object_create(boxed(prototype));
    let child = js_object_create(middle);
    let child_ptr = crate::value::js_nanbox_get_pointer(child) as *mut ObjectHeader;
    assert_eq!(
        unsafe { try_data_get_by_name(child_ptr, key("value")) }
            .unwrap()
            .bits(),
        7.0f64.to_bits()
    );
    differential(child, "value", 7.0);
    js_object_set_field_by_name(prototype, key("value"), 11.0);
    differential(child, "value", 11.0);
    js_object_set_field_by_name(child_ptr, key("value"), 13.0);
    differential(child, "value", 13.0);
    assert_eq!(js_object_delete_field(child_ptr, key("value")), 1);
    differential(child, "value", 11.0);
    js_object_freeze(boxed(prototype));
    assert!(unsafe { try_data_get_by_name(child_ptr, key("value")) }.is_some());
    differential(child, "value", 11.0);
    let replacement = object("value", 17.0);
    crate::object::prototype_chain::object_set_user_prototype(
        child_ptr as usize,
        boxed(replacement).to_bits(),
    );
    differential(child, "value", 17.0);
}

#[test]
fn an_own_absence_proof_is_not_searched_again_before_the_holder() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let prototype = object("value", 7.0);
    let child = js_object_create(boxed(prototype));
    let child_ptr = crate::value::js_nanbox_get_pointer(child) as *mut ObjectHeader;
    js_object_set_field_by_name(child_ptr, key("other"), 1.0);
    let shape = unsafe {
        shapes::own_data_shape(shapes::ordinary_dir_addr(), (*child_ptr).shape_word())
            .flatten()
            .unwrap()
    };
    let before = crate::string::test_key_byte_reads();
    assert_eq!(unsafe { shape.plain_lookup(0, b"value") }, Some(None));
    let absence_reads = crate::string::test_key_byte_reads() - before;
    assert!(absence_reads > 0, "the absence fixture must search a key");
    let before = crate::string::test_key_byte_reads();
    assert_eq!(
        unsafe { try_data_get_by_name(child_ptr, key("value")) }
            .unwrap()
            .bits(),
        7.0f64.to_bits()
    );
    let child_reads = crate::string::test_key_byte_reads() - before;
    let before = crate::string::test_key_byte_reads();
    assert_eq!(
        unsafe { try_data_get_by_name(prototype, key("value")) }
            .unwrap()
            .bits(),
        7.0f64.to_bits()
    );
    let holder_reads = crate::string::test_key_byte_reads() - before;
    // The inherited byte lookup reads the holder's key once. Sabotaging
    // own_absent adds another full receiver search to this exact count.
    assert_eq!(child_reads, absence_reads + holder_reads);
}

#[test]
fn a_default_shape_follows_the_resolved_realm_prototype_without_class_metadata() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let proto = crate::object::builtin_prototype_value("Object");
    let ptr = crate::value::js_nanbox_get_pointer(proto) as *mut ObjectHeader;
    assert!(!ptr.is_null());
    js_object_set_field_by_name(ptr, key("shape_read_test"), 31.0);
    let child = js_object_alloc(0, 0);
    assert_eq!(
        unsafe { shapes::object_shape_identity(child) },
        shapes::PROTO_ID_DEFAULT
    );
    assert_eq!(
        unsafe { try_data_get_by_name(child, key("shape_read_test")) }
            .unwrap()
            .bits(),
        31.0f64.to_bits()
    );
    assert_eq!(js_object_delete_field(ptr, key("shape_read_test")), 1);
    assert!(matches!(
        unsafe {
            try_data_lookup_bytes(
                JSValue::from_bits(boxed(child).to_bits()),
                b"shape_read_test",
            )
        },
        Some(None)
    ));
}

static GETTER_CALLS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn getter(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    GETTER_CALLS.fetch_add(1, Ordering::Relaxed);
    47.0
}

fn install_getter(object: *mut ObjectHeader, name: &str, has_getter: bool) {
    let closure = crate::closure::js_closure_alloc(crate::fn_info!(getter, 0), 0);
    crate::object::descriptor_state::set_accessor_descriptor(
        object as usize,
        name.to_string(),
        crate::object::descriptor_state::AccessorDescriptor {
            get: if has_getter {
                crate::value::js_nanbox_pointer(closure as i64).to_bits()
            } else {
                0
            },
            set: 0,
        },
    );
}

#[test]
fn accessor_bloom_guard_preserves_own_and_inherited_getter_calls() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    GETTER_CALLS.store(0, Ordering::Relaxed);
    let prototype = object("value", 1.0);
    install_getter(prototype, "value", true);
    let child = js_object_create(boxed(prototype));
    for receiver in [boxed(prototype), child] {
        assert!(
            unsafe { try_data_get_bytes(JSValue::from_bits(receiver.to_bits()), b"value") }
                .is_none()
        );
        differential(receiver, "value", 47.0);
    }
    assert_eq!(
        GETTER_CALLS.load(Ordering::Relaxed),
        4,
        "one getter invocation per Get"
    );
    let child_ptr = crate::value::js_nanbox_get_pointer(child) as *mut ObjectHeader;
    js_object_set_field_by_name(child_ptr, key("own"), 53.0);
    differential(child, "own", 53.0);
    assert_eq!(GETTER_CALLS.load(Ordering::Relaxed), 4);
    install_getter(prototype, "value", false);
    differential(
        boxed(prototype),
        "value",
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
}

#[test]
fn unsupported_receivers_and_private_names_decline_and_preserve_results() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let target = object("value", 59.0);
    let handler = js_object_alloc(0, 0);
    let proxy = crate::proxy::js_proxy_new(boxed(target), boxed(handler));
    let array = crate::array::js_array_alloc(4);
    let array_value = crate::value::js_nanbox_pointer(array as i64);
    js_object_set_field_by_name(array as *mut ObjectHeader, key("value"), 59.0);
    let closure = crate::closure::js_closure_alloc(crate::fn_info!(getter, 0), 0);
    js_object_set_field_by_name(closure as *mut ObjectHeader, key("value"), 59.0);
    for receiver in [
        proxy,
        array_value,
        crate::value::js_nanbox_pointer(closure as i64),
    ] {
        assert!(
            unsafe { try_data_get_bytes(JSValue::from_bits(receiver.to_bits()), b"value") }
                .is_none()
        );
        differential(receiver, "value", 59.0);
    }
    let private = object("#value", 61.0);
    assert!(unsafe { try_data_get_by_name(private, key("#value")) }.is_none());
    differential(boxed(private), "#value", 61.0);
    assert!(unsafe { try_data_get_bytes(JSValue::number(3.5), b"value") }.is_none());
}

#[cfg(feature = "regex-engine")]
#[test]
fn regexp_expandos_are_ordinary_data_and_accessors_preserve_observability() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let regexp = crate::regex::js_regexp_new(key("x"), key("g"));
    let regexp_obj = regexp as *mut ObjectHeader;
    js_object_set_field_by_name(regexp_obj, key("value"), 67.0);
    assert_eq!(
        unsafe { try_data_get_by_name(regexp_obj, key("value")) }
            .unwrap()
            .as_number(),
        67.0
    );
    differential(boxed(regexp_obj), "value", 67.0);
    GETTER_CALLS.store(0, Ordering::Relaxed);
    install_getter(regexp_obj, "value", true);
    differential(boxed(regexp_obj), "value", 47.0);
    assert_eq!(GETTER_CALLS.load(Ordering::Relaxed), 2);
}

#[test]
fn borrowed_long_and_inline_keys_match_without_materializing_for_the_fast_read() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    for name in [
        "x",
        "a property name longer than the inline copy buffer; keep borrowing through this lookup",
    ] {
        let object = object(name, 71.0);
        assert_eq!(
            unsafe {
                try_data_get_bytes(JSValue::from_bits(boxed(object).to_bits()), name.as_bytes())
            }
            .unwrap()
            .bits(),
            71.0f64.to_bits()
        );
        differential(boxed(object), name, 71.0);
    }
}

#[test]
fn wide_spilled_and_deleted_keys_match_forced_slow() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let object = js_object_alloc(0, 0);
    for i in 0..100 {
        js_object_set_field_by_name(object, key(&format!("field{i}")), i as f64);
    }
    for i in [0, 4, 50, 99] {
        let name = format!("field{i}");
        assert!(unsafe { try_data_get_by_name(object, key(&name)) }.is_some());
        differential(boxed(object), &name, i as f64);
        assert_eq!(js_object_delete_field(object, key(&name)), 1);
        assert!(unsafe { try_data_get_by_name(object, key(&name)) }.is_none());
        differential(
            boxed(object),
            &name,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        js_object_set_field_by_name(object, key(&name), 101.0);
        differential(boxed(object), &name, 101.0);
    }
}

#[test]
fn class_statics_buffers_and_mapped_arguments_use_the_slow_path() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let class_id = 0x0065_5234;
        crate::object::js_register_class_id(class_id);
        crate::object::js_class_register_static_field(
            class_id,
            b"value".as_ptr(),
            5,
            73.0,
            std::ptr::null_mut(),
        );
        let class_value = f64::from_bits(crate::value::INT32_TAG | u64::from(class_id));
        assert!(try_data_get_bytes(JSValue::from_bits(class_value.to_bits()), b"value").is_none());
        differential(class_value, "value", 73.0);

        let buffer = crate::buffer::js_buffer_alloc(8, 11);
        let buffer_value = crate::value::js_nanbox_pointer(buffer as i64);
        assert!(
            try_data_get_bytes(JSValue::from_bits(buffer_value.to_bits()), b"length").is_none()
        );
        differential(buffer_value, "length", 8.0);

        let args = crate::array::js_array_alloc(1);
        crate::array::js_array_push(args, JSValue::number(1.0));
        let arguments = crate::object::js_arguments_object_alloc_mapped(
            crate::value::js_nanbox_pointer(args as i64),
            f64::from_bits(crate::value::TAG_UNDEFINED),
            1,
        );
        let cell = crate::r#box::js_box_alloc(79.0);
        crate::object::js_arguments_object_map_index(arguments, 0, cell);
        assert!(try_data_get_by_name(arguments, key("0")).is_none());
        differential(boxed(arguments), "0", 79.0);
    }
}

#[test]
fn wide_private_shape_keeps_public_and_private_names_separate() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let object = object("visible", 7.0);
    for i in 0..crate::object::KEYS_INDEX_THRESHOLD {
        js_object_set_field_by_name(object, key(&format!("field{i}")), i as f64);
    }
    unsafe {
        crate::object::key_attrs::apply_edits(
            object,
            &[crate::object::key_attrs::AttrsEdit::Private(b"hidden")],
        );
        let keys = crate::object::object_keys(object);
        // Exercise both the unindexed fallback and the complete shape index.
        for build in [false, true] {
            if build {
                let hash = super::super::key_bytes_hash(b"visible".as_ptr(), 7);
                let _ = shapes::shape_slot_lookup_verdict(
                    keys.arr(),
                    b"visible",
                    hash,
                    keys.count(),
                    true,
                );
            }
            differential(boxed(object), "visible", 7.0);
            differential(
                boxed(object),
                "hidden",
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
        }
    }
}

/// A runtime-born ordinary plain object, as every literal and `{}` birth site
/// marks it; `js_object_alloc` alone leaves the mark to its caller.
fn plain_object(name: &str, value: f64) -> *mut ObjectHeader {
    let object = js_object_alloc(0, 0);
    unsafe { crate::object::mark_object_plain_ordinary(object) };
    js_object_set_field_by_name(object, key(name), value);
    object
}

/// An instance of an object literal's synthetic anon-shape class, the way
/// codegen births `{}` and `{ a: 1 }`: a nonzero class id, no birth mark.
fn anon_shape_object(class_id: u32) -> *mut ObjectHeader {
    unsafe { crate::object::js_register_anon_shape_class_id(class_id) };
    crate::object::js_object_alloc_class_with_keys(class_id, 0, 0, std::ptr::null(), 0)
}

fn marked(receiver: f64) -> f64 {
    unsafe {
        crate::object::mark_object_plain_ordinary(
            crate::value::js_nanbox_get_pointer(receiver) as *mut ObjectHeader
        )
    };
    receiver
}

fn by_name(receiver: f64, name: &str) -> u64 {
    crate::object::js_object_get_field_by_name(receiver.to_bits() as *const ObjectHeader, key(name))
        .bits()
}

const PROBE_KEYS: &[&str] = &[
    "a",
    "missing",
    "toString",
    "hasOwnProperty",
    "valueOf",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "__defineGetter__",
    "__lookupGetter__",
    "__proto__",
    "constructor",
    "0",
    "1",
    "length",
    "name",
    "flags",
    "callee",
    "@@iterator",
    "@@toStringTag",
    "#private",
];

/// The named read must settle to exactly what the generic ladder returns, for
/// every probe key, with the settled lanes on and with every shape lane off.
fn settled_matches_generic(receiver: f64, label: &str) {
    for name in PROBE_KEYS {
        let settled = by_name(receiver, name);
        let generic = {
            let _slow = Slow::enter();
            by_name(receiver, name)
        };
        assert!(
            settled == generic || same_string(settled, generic),
            "{label}: settled read of {name:?} diverged from the generic ladder \
             ({settled:#x} vs {generic:#x})"
        );
    }
}

/// Two reads of one string-valued property may materialize distinct cells.
fn same_string(a: u64, b: u64) -> bool {
    let (a, b) = (JSValue::from_bits(a), JSValue::from_bits(b));
    (a.is_string() || a.is_short_string())
        && (b.is_string() || b.is_short_string())
        && crate::string::js_string_equals_value(f64::from_bits(a.bits()), f64::from_bits(b.bits()))
            != 0
}

#[test]
fn settled_named_reads_match_the_generic_ladder_across_receiver_kinds() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    // Resolve the realm Object.prototype so the terminal shortcuts can engage.
    let object_prototype = crate::object::builtin_prototype_value("Object");
    let object_prototype_ptr =
        crate::value::js_nanbox_get_pointer(object_prototype) as *mut ObjectHeader;
    assert!(!object_prototype_ptr.is_null());

    let plain = plain_object("a", 1.0);
    js_object_set_field_by_name(plain, key("0"), 2.0);
    let empty = js_object_alloc(0, 0);
    marked(boxed(empty));
    let null_proto = marked(js_object_create(f64::from_bits(crate::value::TAG_NULL)));
    let child = marked(js_object_create(boxed(plain)));
    let grandchild = marked(js_object_create(child));
    let with_getter = plain_object("a", 1.0);
    install_getter(with_getter, "missing", true);
    let anon = anon_shape_object(0x5A00_0001);
    js_object_set_field_by_name(anon, key("a"), 1.0);
    let anon_empty = anon_shape_object(0x5A00_0002);
    let anon_getter = anon_shape_object(0x5A00_0003);
    install_getter(anon_getter, "missing", true);
    let wide = js_object_alloc(0, 0);
    marked(boxed(wide));
    for i in 0..100 {
        js_object_set_field_by_name(wide, key(&format!("field{i}")), i as f64);
    }
    let array = crate::array::js_array_alloc(4);
    let closure = crate::closure::js_closure_alloc(crate::fn_info!(getter, 0), 0);
    let weakmap = crate::weakref::js_weakmap_new();
    let weakset = crate::weakref::js_weakset_new();
    marked(boxed(weakmap));
    marked(boxed(weakset));
    let map = crate::map::js_map_alloc(4);
    let set = crate::set::js_set_alloc(4);
    let args = crate::array::js_array_alloc(1);
    crate::array::js_array_push(args, JSValue::number(1.0));
    let arguments = crate::object::js_arguments_object_alloc_mapped(
        crate::value::js_nanbox_pointer(args as i64),
        f64::from_bits(crate::value::TAG_UNDEFINED),
        1,
    );
    settled_matches_generic(boxed(plain), "plain");
    settled_matches_generic(boxed(empty), "empty");
    settled_matches_generic(null_proto, "Object.create(null)");
    settled_matches_generic(child, "child of plain");
    settled_matches_generic(grandchild, "grandchild of plain");
    settled_matches_generic(boxed(with_getter), "own getter");
    settled_matches_generic(boxed(anon), "anon-shape literal");
    settled_matches_generic(boxed(anon_empty), "anon-shape empty literal");
    settled_matches_generic(boxed(anon_getter), "anon-shape literal with a getter");
    settled_matches_generic(boxed(wide), "wide");
    settled_matches_generic(boxed(object_prototype_ptr), "Object.prototype");
    settled_matches_generic(crate::value::js_nanbox_pointer(array as i64), "array");
    settled_matches_generic(crate::value::js_nanbox_pointer(closure as i64), "closure");
    settled_matches_generic(boxed(weakmap), "WeakMap");
    settled_matches_generic(boxed(weakset), "WeakSet");
    settled_matches_generic(crate::value::js_nanbox_pointer(map as i64), "Map");
    settled_matches_generic(crate::value::js_nanbox_pointer(set as i64), "Set");
    settled_matches_generic(boxed(arguments), "arguments");
    #[cfg(feature = "regex-engine")]
    {
        let regexp = crate::regex::js_regexp_new(key("x"), key("g"));
        settled_matches_generic(boxed(regexp as *mut ObjectHeader), "RegExp");
    }
}

#[test]
fn the_settled_miss_is_taken_only_for_plain_ordinary_receivers() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let _ = crate::object::builtin_prototype_value("Object");
    let settled = |object: *mut ObjectHeader, name: &str| unsafe {
        try_settled_get_by_name(object, key(name)).map(|value| value.bits())
    };
    let undefined = crate::value::TAG_UNDEFINED;
    let plain = plain_object("a", 1.0);
    assert_eq!(settled(plain, "a"), Some(1.0f64.to_bits()));
    assert_eq!(settled(plain, "missing"), Some(undefined));
    assert_eq!(settled(plain, "toString").is_some(), true);
    assert_ne!(settled(plain, "toString"), Some(undefined));
    // Symbol-keyed reads stay with the generic ladder.
    assert_eq!(settled(plain, "@@iterator"), None);
    // A miss decided by an own accessor is never settled to undefined here.
    let with_getter = plain_object("a", 1.0);
    install_getter(with_getter, "missing", true);
    assert_eq!(settled(with_getter, "missing"), None);
    // Receivers the shapes alone do not decide.
    let weakmap = crate::weakref::js_weakmap_new();
    marked(boxed(weakmap));
    assert_eq!(settled(weakmap, "missing"), None);
    // A birth that never carried the runtime's plain mark keeps the generic read.
    let unmarked = object("a", 1.0);
    assert_eq!(settled(unmarked, "missing"), None);
    let arguments_args = crate::array::js_array_alloc(0);
    let arguments = crate::object::js_arguments_object_alloc_mapped(
        crate::value::js_nanbox_pointer(arguments_args as i64),
        f64::from_bits(crate::value::TAG_UNDEFINED),
        0,
    );
    assert_eq!(settled(arguments, "missing"), None);
}

#[test]
fn an_anon_shape_literal_settles_until_its_class_gains_a_surface() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let _ = crate::object::builtin_prototype_value("Object");
    let settled = |object: *mut ObjectHeader, name: &str| unsafe {
        try_settled_get_by_name(object, key(name)).map(|value| value.bits())
    };
    let undefined = crate::value::TAG_UNDEFINED;
    let class_id = 0x5A00_0010;
    let literal = anon_shape_object(class_id);
    js_object_set_field_by_name(literal, key("a"), 1.0);
    assert_eq!(settled(literal, "a"), Some(1.0f64.to_bits()));
    assert_eq!(settled(literal, "missing"), Some(undefined));
    assert_ne!(settled(literal, "toString"), Some(undefined));
    // An id that is a declared class (a name and a method) keeps the generic
    // read, even when an anon-shape id once had the same number.
    unsafe {
        crate::object::js_register_class_name(class_id, b"Declared".as_ptr(), 8);
        crate::object::js_register_class_method(
            class_id as i64,
            b"run".as_ptr(),
            3,
            0 as usize as i64,
            0,
            0,
            0,
        );
    }
    assert_eq!(settled(literal, "missing"), None);
    // A class id nothing registered as an anon shape never settles.
    let foreign =
        crate::object::js_object_alloc_class_with_keys(0x5A00_0011, 0, 0, std::ptr::null(), 0);
    assert_eq!(settled(foreign, "missing"), None);
}

#[test]
fn object_prototype_mutation_after_a_settled_miss_is_observed() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let object_prototype = crate::object::builtin_prototype_value("Object");
    let proto_ptr = crate::value::js_nanbox_get_pointer(object_prototype) as *mut ObjectHeader;
    let undefined = crate::value::TAG_UNDEFINED;
    let plain = plain_object("a", 1.0);
    let child = marked(js_object_create(boxed(plain)));
    let child_ptr = crate::value::js_nanbox_get_pointer(child) as *mut ObjectHeader;
    let name = "settled_probe_added_later";
    for receiver in [boxed(plain), child] {
        assert_eq!(by_name(receiver, name), undefined);
    }
    // A data property added to Object.prototype is inherited by every chain.
    js_object_set_field_by_name(proto_ptr, key(name), 5.0);
    for receiver in [boxed(plain), child] {
        assert_eq!(by_name(receiver, name), 5.0f64.to_bits());
        let _slow = Slow::enter();
        assert_eq!(by_name(receiver, name), 5.0f64.to_bits());
    }
    // An own property shadows it; deleting the own property reveals it again.
    js_object_set_field_by_name(child_ptr, key(name), 6.0);
    assert_eq!(by_name(child, name), 6.0f64.to_bits());
    assert_eq!(js_object_delete_field(child_ptr, key(name)), 1);
    assert_eq!(by_name(child, name), 5.0f64.to_bits());
    // Deleting the Object.prototype property returns the settled miss.
    assert_eq!(js_object_delete_field(proto_ptr, key(name)), 1);
    for receiver in [boxed(plain), child] {
        assert_eq!(by_name(receiver, name), undefined);
        let _slow = Slow::enter();
        assert_eq!(by_name(receiver, name), undefined);
    }
    // An accessor added to Object.prototype runs for a receiver that missed.
    GETTER_CALLS.store(0, Ordering::Relaxed);
    install_getter(proto_ptr, name, true);
    for receiver in [boxed(plain), child] {
        assert_eq!(by_name(receiver, name), 47.0f64.to_bits());
    }
    assert_eq!(GETTER_CALLS.load(Ordering::Relaxed), 2);
    install_getter(proto_ptr, name, false);
}

#[test]
fn deleting_and_re_adding_keys_keeps_the_settled_miss_exact() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let _ = crate::object::builtin_prototype_value("Object");
    let undefined = crate::value::TAG_UNDEFINED;
    let object = plain_object("a", 1.0);
    js_object_set_field_by_name(object, key("b"), 2.0);
    for _ in 0..3 {
        assert_eq!(by_name(boxed(object), "b"), 2.0f64.to_bits());
        assert_eq!(js_object_delete_field(object, key("b")), 1);
        assert_eq!(by_name(boxed(object), "b"), undefined);
        assert_eq!(by_name(boxed(object), "a"), 1.0f64.to_bits());
        js_object_set_field_by_name(object, key("b"), 2.0);
    }
    // A prototype change after a miss is observed.
    let holder = self::object("late", 9.0);
    assert_eq!(by_name(boxed(object), "late"), undefined);
    crate::object::prototype_chain::object_set_user_prototype(
        object as usize,
        boxed(holder).to_bits(),
    );
    assert_eq!(by_name(boxed(object), "late"), 9.0f64.to_bits());
    crate::object::prototype_chain::object_set_user_prototype(
        object as usize,
        f64::from_bits(crate::value::TAG_NULL).to_bits(),
    );
    assert_eq!(by_name(boxed(object), "late"), undefined);
    assert_eq!(by_name(boxed(object), "toString"), undefined);
}
