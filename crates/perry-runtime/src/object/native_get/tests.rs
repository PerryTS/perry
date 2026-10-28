use super::*;
use crate::codegen_abi::LayoutWord;
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
        shapes::own_data_shape(
            shapes::ordinary_dir_addr(),
            LayoutWord((*object).shape_word()),
        )
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
        shapes::own_data_shape(
            shapes::ordinary_dir_addr(),
            LayoutWord((*child_ptr).shape_word()),
        )
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
        unsafe { shapes::object_proto_identity(child) },
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
    let proxy = crate::proxy_hooks::proxy_new(boxed(target), boxed(handler));
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
    unsafe { crate::object::class_registry::declarations::test_declare_anonymous(class_id) };
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
fn an_anon_shape_literal_keeps_its_role_and_a_declared_class_does_not_settle() {
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
    // Cross-role reuse is rejected without changing the literal's owner.
    assert!(
        crate::object::class_registry::declarations::register_declared_identity(class_id).is_err()
    );
    assert_eq!(settled(literal, "missing"), Some(undefined));
    // A distinct declared class keeps the generic read, even when empty.
    let declared_id = 0x5A00_0011;
    unsafe {
        crate::object::js_register_class_name(declared_id, b"Declared".as_ptr(), 8);
        crate::object::js_register_class_method(
            declared_id as i64,
            b"run".as_ptr(),
            3,
            0 as usize as i64,
            0,
            0,
            0,
        );
    }
    let declared =
        crate::object::js_object_alloc_class_with_keys(declared_id, 0, 0, std::ptr::null(), 0);
    assert_eq!(settled(declared, "missing"), None);
    // An unregistered identity is not a compiler-proven anonymous record.
    let foreign =
        crate::object::js_object_alloc_class_with_keys(0x5A00_0012, 0, 0, std::ptr::null(), 0);
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

extern "C" fn class_method(_this: f64) -> f64 {
    1.0
}

/// A declared class (a name and one method with a compiled entry) in the
/// compiled-id band, as codegen registers it.
fn declared_class(class_id: u32, method: &str, parent: Option<u32>) {
    unsafe {
        crate::object::js_register_class_name(class_id, b"Declared".as_ptr(), 8);
        crate::object::class_registry::js_register_class_method_with_entry(
            class_id as i64,
            method.as_ptr(),
            method.len() as i64,
            class_method as *const () as usize as i64,
            0,
            0,
            0,
            0,
        );
    }
    if let Some(parent) = parent {
        crate::object::class_registry::js_register_class_parent(class_id, parent);
    }
}

fn declared_prototype(class_id: u32) -> *mut ObjectHeader {
    crate::value::js_nanbox_get_pointer(crate::object::class_registry::class_decl_prototype_value(
        class_id,
    )) as *mut ObjectHeader
}

fn settled_bits(object: *mut ObjectHeader, name: &str) -> Option<u64> {
    unsafe { try_settled_get_by_name(object, key(name)).map(|value| value.bits()) }
}

#[test]
fn a_declared_class_instance_settles_from_its_chain_and_matches_the_generic_read() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let _ = crate::object::builtin_prototype_value("Object");
    let undefined = crate::value::TAG_UNDEFINED;
    let (base, derived) = (0x7C00, 0x7C01);
    declared_class(base, "m", None);
    declared_class(derived, "n", Some(base));
    let instance = crate::object::js_object_alloc(derived, 0);
    js_object_set_field_by_name(instance, key("a"), 1.0);
    assert_eq!(settled_bits(instance, "a"), Some(1.0f64.to_bits()));
    assert_eq!(by_name(boxed(instance), "missing"), undefined);
    declared_prototype(base);
    declared_prototype(derived);
    // Now a name no link lists settles to undefined, and an Object.prototype
    // data property to its value; a class member never settles (the generic
    // read binds it).
    assert_eq!(settled_bits(instance, "missing"), Some(undefined));
    assert_eq!(settled_bits(instance, "stop"), Some(undefined));
    let to_string = settled_bits(instance, "toString").expect("Object.prototype data");
    assert_ne!(to_string, undefined);
    assert_eq!(to_string, by_name(boxed(instance), "toString"));
    for member in ["m", "n", "constructor", "__proto__", "@@iterator", "#p"] {
        assert_eq!(settled_bits(instance, member), None, "{member}");
    }
    settled_matches_generic(boxed(instance), "declared class instance");
    let base_instance = crate::object::js_object_alloc(base, 0);
    settled_matches_generic(boxed(base_instance), "declared base class instance");
}

#[test]
fn a_declared_class_settled_miss_follows_prototype_mutation() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let object_prototype = crate::object::builtin_prototype_value("Object");
    let object_prototype_ptr =
        crate::value::js_nanbox_get_pointer(object_prototype) as *mut ObjectHeader;
    let undefined = crate::value::TAG_UNDEFINED;
    let (base, derived) = (0x7C10, 0x7C11);
    declared_class(base, "m", None);
    declared_class(derived, "n", Some(base));
    let instance = crate::object::js_object_alloc(derived, 0);
    let base_proto = declared_prototype(base);
    let derived_proto = declared_prototype(derived);
    let name = "class_settled_probe_added_later";
    assert_eq!(settled_bits(instance, name), Some(undefined));
    // A data property added to a class prototype after the miss settled.
    js_object_set_field_by_name(base_proto, key(name), 5.0);
    assert_eq!(settled_bits(instance, name), None);
    assert_eq!(by_name(boxed(instance), name), 5.0f64.to_bits());
    assert_eq!(js_object_delete_field(base_proto, key(name)), 1);
    // A deleted key may leave the holder's shape unsettled; it must never
    // settle to anything but the generic answer.
    assert!(settled_bits(instance, name).is_none_or(|bits| bits == undefined));
    assert_eq!(by_name(boxed(instance), name), undefined);
    // A getter defined on a class prototype after the miss settled.
    GETTER_CALLS.store(0, Ordering::Relaxed);
    install_getter(derived_proto, name, true);
    assert_eq!(settled_bits(instance, name), None);
    assert_eq!(by_name(boxed(instance), name), 47.0f64.to_bits());
    assert_eq!(GETTER_CALLS.load(Ordering::Relaxed), 1);
    install_getter(derived_proto, name, false);
    // Object.prototype gains and loses the name.
    let other = "class_settled_probe_on_object_prototype";
    assert_eq!(settled_bits(instance, other), Some(undefined));
    js_object_set_field_by_name(object_prototype_ptr, key(other), 7.0);
    assert_eq!(settled_bits(instance, other), Some(7.0f64.to_bits()));
    assert_eq!(by_name(boxed(instance), other), 7.0f64.to_bits());
    assert_eq!(js_object_delete_field(object_prototype_ptr, key(other)), 1);
    assert_eq!(settled_bits(instance, other), Some(undefined));
    // An own property of the instance shadows every holder.
    js_object_set_field_by_name(instance, key(other), 8.0);
    assert_eq!(settled_bits(instance, other), Some(8.0f64.to_bits()));
}

#[test]
fn a_relinked_prototype_or_instance_leaves_the_declared_class_settled_miss() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let _ = crate::object::builtin_prototype_value("Object");
    let undefined = crate::value::TAG_UNDEFINED;
    let late = "class_relink_probe";
    // `Object.setPrototypeOf(instance, other)`.
    let (a, b) = (0x7C20, 0x7C21);
    declared_class(a, "m", None);
    declared_class(b, "n", Some(a));
    let instance = crate::object::js_object_alloc(b, 0);
    declared_prototype(a);
    declared_prototype(b);
    assert_eq!(settled_bits(instance, late), Some(undefined));
    let donor = object(late, 9.0);
    crate::object::prototype_chain::object_set_user_prototype(
        instance as usize,
        boxed(donor).to_bits(),
    );
    assert_eq!(settled_bits(instance, late), None);
    assert_eq!(by_name(boxed(instance), late), 9.0f64.to_bits());
    crate::object::prototype_chain::object_set_user_prototype(
        instance as usize,
        f64::from_bits(crate::value::TAG_NULL).to_bits(),
    );
    assert_eq!(settled_bits(instance, "toString"), None);
    assert_eq!(by_name(boxed(instance), late), undefined);
    // `Object.setPrototypeOf(Derived.prototype, other)`.
    let (c, d) = (0x7C22, 0x7C23);
    declared_class(c, "m", None);
    declared_class(d, "n", Some(c));
    let other_instance = crate::object::js_object_alloc(d, 0);
    declared_prototype(c);
    let d_proto = declared_prototype(d);
    assert_eq!(settled_bits(other_instance, late), Some(undefined));
    crate::object::prototype_chain::object_set_user_prototype(
        d_proto as usize,
        boxed(donor).to_bits(),
    );
    assert_eq!(settled_bits(other_instance, late), None);
    assert_eq!(by_name(boxed(other_instance), late), 9.0f64.to_bits());
}

#[test]
fn a_class_extending_a_native_never_settles() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let _ = crate::object::builtin_prototype_value("Object");
    let class_id = 0x7C30;
    declared_class(class_id, "m", None);
    // The reserved native band codegen registers for `extends Map`.
    crate::object::class_registry::js_register_class_parent(class_id, 0xFFFF_0001);
    let instance = crate::object::js_object_alloc(class_id, 0);
    declared_prototype(class_id);
    assert_eq!(settled_bits(instance, "missing"), None);
    assert_eq!(settled_bits(instance, "toString"), None);
}

/// A native registry id in the id band, as `new Headers()` hands one out.
const RAW_NATIVE_ID: usize = crate::value::addr_class::FETCH_HANDLE_BAND_START;

unsafe extern "C" fn raw_id_property(handle: i64, key: *const u8, len: usize) -> f64 {
    if handle as usize == RAW_NATIVE_ID && std::slice::from_raw_parts(key, len) == b"length" {
        42.0
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

/// `js_object_get_field_by_name`'s typed-array probe takes the raw ABI
/// receiver. A native id must box through the unknown-word constructor (never
/// the heap-only `JSValue::pointer`, which a contract build rejects) and still
/// reach the native-handle property dispatch, for a metadata name the probe
/// reads and for an ordinary miss.
#[test]
fn raw_native_id_receiver_reaches_handle_dispatch_past_the_typed_array_probe() {
    let prev = crate::object::class_handles::swap_handle_property_dispatch_for_test(
        raw_id_property as *mut (),
    );
    let receiver = RAW_NATIVE_ID as *const ObjectHeader;
    let length = crate::object::js_object_get_field_by_name(receiver, key("length"));
    let missing = crate::object::js_object_get_field_by_name(receiver, key("missing"));
    crate::object::class_handles::swap_handle_property_dispatch_for_test(prev);
    assert_eq!(f64::from_bits(length.bits()), 42.0);
    assert_eq!(missing.bits(), crate::value::TAG_UNDEFINED);
}

/// The settled data lane's raw receiver: a native id has no shape, so the lane
/// declines (`None`) rather than answering a definite miss, and it boxes the id
/// through the unknown-word constructor.
#[test]
fn settled_data_lane_declines_a_raw_native_id_receiver() {
    let receiver = RAW_NATIVE_ID as *const ObjectHeader;
    unsafe {
        assert!(try_data_lookup_by_name(receiver, key("missing")).is_none());
        assert!(try_settled_get_by_name(receiver, key("length")).is_none());
    }
}

static RAW_ID_SET_HITS: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn raw_id_property_set(handle: i64, key: *const u8, len: usize, value: f64) {
    if handle as usize == RAW_NATIVE_ID
        && std::slice::from_raw_parts(key, len) == b"missing"
        && value == 3.0
    {
        RAW_ID_SET_HITS.fetch_add(1, Ordering::SeqCst);
    }
}

/// The polymorphic keyed setter (`receiver[k] ??= v`) gets the receiver word
/// unclassified. A native id, bare or `HANDLE_TAG`-boxed, is no allocation, so
/// it must not be read as an array header (unmapped memory); it reaches the
/// by-name setter's handle-property-set dispatch.
#[test]
fn keyed_setter_routes_a_raw_native_id_receiver_to_handle_set_dispatch() {
    let prev = crate::object::class_handles::swap_handle_property_set_dispatch_for_test(
        raw_id_property_set as *mut (),
    );
    let before = RAW_ID_SET_HITS.load(Ordering::SeqCst);
    // Both words codegen hands this setter for a handle: the bare id and the
    // `HANDLE_TAG` value of an `any` binding.
    let raw = RAW_NATIVE_ID as *mut crate::array::ArrayHeader;
    let tagged = crate::value::JSValue::handle(RAW_NATIVE_ID).bits() as usize
        as *mut crate::array::ArrayHeader;
    let returned_raw = crate::array::js_array_set_string_key(raw, key("missing"), 3.0);
    let returned_tagged = crate::array::js_array_set_string_key(tagged, key("missing"), 3.0);
    let hits = RAW_ID_SET_HITS.load(Ordering::SeqCst) - before;
    crate::object::class_handles::swap_handle_property_set_dispatch_for_test(prev);
    assert_eq!(returned_raw, raw);
    assert_eq!(returned_tagged, tagged);
    assert_eq!(hits, 2);
}
