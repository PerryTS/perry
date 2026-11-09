//! LAZYPROTO P1: builtin families born on first observation
//! (`BuiltinConstructorDeclaration::lazy`).
use super::*;

fn lazy_declarations() -> Vec<&'static BuiltinConstructorDeclaration> {
    GLOBAL_THIS_BUILTIN_CONSTRUCTORS
        .iter()
        .filter(|declaration| declaration.lazy)
        .collect()
}

/// `value`'s own string keys, through `Object.getOwnPropertyNames`.
fn own_names(value: f64) -> Vec<String> {
    let names = super::super::js_object_get_own_property_names(value);
    let array = crate::value::js_nanbox_get_pointer(names) as *const crate::array::ArrayHeader;
    (0..crate::array::js_array_length(array))
        .map(|index| {
            let key = crate::array::js_array_get_f64_unchecked(array, index);
            super::super::property_ops::set::key_to_rust_string(key).unwrap()
        })
        .collect()
}

fn constructor(declaration: &BuiltinConstructorDeclaration) -> f64 {
    js_get_global_this_builtin_value(declaration.name.as_ptr(), declaration.name.len())
}

/// Fixture 4: realm setup allocates and binds a lazy family's constructor
/// and births nothing else; the first ask for its prototype births it.
#[test]
fn realm_population_does_not_birth_lazy_families() {
    let _lock = crate::gc::global_side_table_test_lock();
    js_get_global_this();
    let lazy = lazy_declarations();
    assert!(!lazy.is_empty(), "Navigator is declared lazy");
    for declaration in &lazy {
        let ctor = crate::array::intrinsic_constructor_addr_if_bound(declaration.identity);
        assert_ne!(ctor, 0, "{}: setup binds the constructor", declaration.name);
        assert_eq!(
            crate::value::js_nanbox_get_pointer(constructor(declaration)) as usize,
            ctor,
            "{}: globalThis binds the realm's constructor",
            declaration.name
        );
        assert_eq!(
            crate::array::intrinsic_prototype_addr_if_bound(declaration.identity),
            0,
            "{} was born at realm setup",
            declaration.name
        );
        // SAFETY: the realm's live constructor closure.
        assert!(
            unsafe { crate::closure::props::bag_of(ctor) }.is_null(),
            "{}: an unborn constructor has no bag",
            declaration.name
        );
    }
    for declaration in &lazy {
        let proto = builtin_prototype_value(declaration.name);
        assert!(JSValue::from_bits(proto.to_bits()).is_pointer());
        assert_eq!(
            crate::array::intrinsic_prototype_addr_if_bound(declaration.identity),
            crate::value::js_nanbox_get_pointer(proto) as usize,
            "{}: the first ask births the family",
            declaration.name
        );
    }
}

/// Fixture 5: every `lazy` declaration can be born late. Its constructor
/// owns `length`, `name` and `prototype` first, the realm seal does not name it (the seal would read its
/// prototype at setup), and no setup step reached its prototype (the
/// population test above). Listing a bagless
/// constructor's keys births its family.
#[test]
fn lazy_declarations_are_bornable_late() {
    let _lock = crate::gc::global_side_table_test_lock();
    js_get_global_this();
    for declaration in lazy_declarations() {
        assert!(
            !crate::object::builtin_proto::realm_seals_constructor_prototype(declaration),
            "{}: seal_realm seals it at setup",
            declaration.name
        );
        // Its prototype publishes no identity word (class, serial, sealed or
        // named serial): such a word reads 0 until the birth, and its readers
        // are audited per family before one may be lazy (P1-AUDIT).
        assert!(
            declaration.prototype_class.is_none()
                && declaration.prototype_serial.is_none()
                && declaration.sealed_serial.is_none()
                && declaration.named_serial.is_none(),
            "{}: its prototype names an identity word",
            declaration.name
        );
        let before = own_names(constructor(declaration));
        // Listing a constructor's keys is an observation of it: its statics
        // (ReadableStream.from) belong to the birth, so the listing births.
        assert_ne!(
            crate::array::intrinsic_prototype_addr_if_bound(declaration.identity),
            0,
            "{}: listing its constructor's keys must birth it",
            declaration.name
        );
        assert_eq!(before[..3], ["length", "name", "prototype"], "{}", declaration.name);
        builtin_prototype_value(declaration.name);
        assert_eq!(
            own_names(constructor(declaration)),
            before,
            "{}: the birth gave the constructor its keys once",
            declaration.name
        );
    }
}

/// The prototype's and the constructor's own keys, and the constructor's
/// `name`, after `first` births the family in a fresh realm (a new agent
/// thread).
fn keys_after_first_touch(first: fn(f64)) -> (Vec<String>, Vec<String>, String) {
    // The program-selected getters (`navigator` install) are part of the birth.
    crate::feature_hooks::js_runtime_install_navigator();
    std::thread::spawn(move || {
        js_get_global_this();
        let ctor = js_get_global_this_builtin_value(b"Navigator".as_ptr(), 9);
        first(ctor);
        let proto = builtin_prototype_value("Navigator");
        let key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
        let name = js_object_get_field_by_name_f64(
            crate::value::js_nanbox_get_pointer(ctor) as *const ObjectHeader,
            key,
        );
        let name = super::super::property_ops::set::key_to_rust_string(name).unwrap();
        (own_names(proto), own_names(ctor), name)
    })
    .join()
    .unwrap()
}

/// Fixture 2: the family's keys are the declaration's whatever observation
/// births it: the intrinsic ask, a write to the constructor (its bag), an
/// instance, or the `prototype` read.
#[test]
fn lazy_family_keys_do_not_depend_on_the_first_touch() {
    let _lock = crate::gc::global_side_table_test_lock();
    let by_intrinsic = keys_after_first_touch(|_| {
        builtin_prototype_value("Navigator");
    });
    // A define of the constructor's own `name` before the birth (it needs the
    // bag): the birth comes first, so the define is not undone by it.
    let by_write = keys_after_first_touch(|ctor| {
        let ptr = crate::value::js_nanbox_get_pointer(ctor) as usize;
        let renamed = crate::string::js_string_from_bytes(b"Renamed".as_ptr(), 7);
        super::super::define_builtin_data_property(
            ptr as *mut ObjectHeader,
            crate::string::js_string_from_bytes(b"name".as_ptr(), 4),
            crate::value::js_nanbox_string(renamed as i64),
            "name".to_string(),
            super::super::PropertyAttrs::new(false, false, true),
        );
        let key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
        js_object_set_field_by_name(ptr as *mut ObjectHeader, key, 1.0);
    });
    let by_instance = keys_after_first_touch(|_| {
        crate::navigator::js_navigator_object();
    });
    let by_read = keys_after_first_touch(|ctor| {
        crate::object::js_function_prototype_value_for_read(ctor);
    });
    assert_eq!(by_intrinsic.1, ["length", "name", "prototype"]);
    assert_eq!(by_write.1, ["length", "name", "prototype", "x"]);
    assert_eq!(by_intrinsic.2, "Navigator");
    assert_eq!(by_write.2, "Renamed", "the birth undid a define made before it");
    for (order, keys) in [("write", &by_write), ("instance", &by_instance), ("read", &by_read)] {
        assert_eq!(keys.0, by_intrinsic.0, "prototype keys, first touch {order}");
        if order != "write" {
            assert_eq!(keys.1, by_intrinsic.1, "constructor keys, first touch {order}");
            assert_eq!(keys.2, by_intrinsic.2, "constructor name, first touch {order}");
        }
    }
    assert_eq!(
        by_intrinsic.0,
        [
            "constructor",
            "hardwareConcurrency",
            "locks",
            "language",
            "languages",
            "userAgent",
            "platform"
        ]
    );
}

/// Fixture 3 (runtime half): enumerating the global object births no lazy
/// family; reading `Navigator.prototype` through the global does.
#[test]
fn global_enumeration_does_not_birth_lazy_families() {
    let _lock = crate::gc::global_side_table_test_lock();
    let global = js_get_global_this();
    let names = own_names(global);
    assert!(names.iter().any(|name| name == "Navigator"));
    let navigator = crate::object::BuiltinConstructorIdentity::Navigator;
    assert_eq!(crate::array::intrinsic_prototype_addr_if_bound(navigator), 0);
    let ctor = js_get_global_this_builtin_value(b"Navigator".as_ptr(), 9);
    let key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    let proto = js_object_get_field_by_name_f64(
        crate::value::js_nanbox_get_pointer(ctor) as *const ObjectHeader,
        key,
    );
    assert_ne!(crate::array::intrinsic_prototype_addr_if_bound(navigator), 0);
    assert_eq!(
        crate::value::js_nanbox_get_pointer(proto) as usize,
        crate::array::intrinsic_prototype_addr_if_bound(navigator)
    );
}

/// LAZYPROTO P2 batch B2: the families declared lazy, by name. Pinned here so
/// that a family forced back to eager fails this test instead of silently
/// costing its setup again.
const B2_LAZY: &[&str] = &[
    "TextEncoder",
    "TextDecoder",
    "TextEncoderStream",
    "TextDecoderStream",
    "CompressionStream",
    "DecompressionStream",
    "WritableStream",
    "TransformStream",
    "URLPattern",
    "MessageEvent",
    "CustomEvent",
    "FormData",
    "Blob",
    "File",
    "Headers",
    "Request",
    "MessageChannel",
    "MessagePort",
    "BroadcastChannel",
    "Crypto",
    "CryptoKey",
    "Storage",
    "DisposableStack",
    "AsyncDisposableStack",
    "SuppressedError",
];

#[test]
fn b2_families_are_lazy_and_unborn_after_setup() {
    let _lock = crate::gc::global_side_table_test_lock();
    js_get_global_this();
    for name in B2_LAZY {
        let identity = crate::object::BuiltinConstructorIdentity::for_name(name)
            .unwrap_or_else(|| panic!("{name} is not a builtin constructor"));
        assert!(identity.declaration().lazy, "{name} is not declared lazy");
        assert_eq!(
            crate::array::intrinsic_prototype_addr_if_bound(identity),
            0,
            "{name} was born at realm setup"
        );
        assert_ne!(
            crate::array::intrinsic_constructor_addr_if_bound(identity),
            0,
            "{name}: setup binds the constructor"
        );
    }
}

/// `length` of an unborn constructor reads as the birth installs it.
#[test]
fn unborn_lazy_constructor_length_is_the_spec_length() {
    let _lock = crate::gc::global_side_table_test_lock();
    js_get_global_this();
    let read_length = |name: &str| {
        let ctor = js_get_global_this_builtin_value(name.as_ptr(), name.len());
        let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
        js_object_get_field_by_name_f64(
            crate::value::js_nanbox_get_pointer(ctor) as *const ObjectHeader,
            key,
        )
    };
    for name in B2_LAZY {
        let identity = crate::object::BuiltinConstructorIdentity::for_name(name).unwrap();
        let unborn = read_length(name);
        assert_eq!(
            crate::array::intrinsic_prototype_addr_if_bound(identity),
            0,
            "{name}: reading length birthed it"
        );
        builtin_prototype_value(name);
        let born = read_length(name);
        assert_eq!(unborn.to_bits(), born.to_bits(), "{name}: length changed at birth");
        if let Some(spec) = crate::object::builtin_constructor_spec_length(name) {
            assert_eq!(born, spec as f64, "{name}: declared length");
        }
    }
}

/// The areas behind `localStorage` / `sessionStorage` are made by the first
/// read of the global (Node's accessors), and that read births `Storage`.
#[test]
fn storage_areas_are_made_by_their_getters() {
    let _lock = crate::gc::global_side_table_test_lock();
    js_get_global_this();
    let storage = crate::object::BuiltinConstructorIdentity::Storage;
    assert_eq!(crate::object::LOCAL_STORAGE_PTR.load(std::sync::atomic::Ordering::Acquire), 0);
    assert_eq!(crate::object::SESSION_STORAGE_PTR.load(std::sync::atomic::Ordering::Acquire), 0);
    assert_eq!(crate::array::intrinsic_prototype_addr_if_bound(storage), 0);
    crate::web_storage::test_read_storage_globals();
    let proto = crate::array::intrinsic_prototype_addr_if_bound(storage);
    assert_ne!(proto, 0, "the first read of the global births Storage");
    for slot in [&crate::object::LOCAL_STORAGE_PTR, &crate::object::SESSION_STORAGE_PTR] {
        let area = slot.load(std::sync::atomic::Ordering::Acquire) as usize;
        assert_ne!(area, 0, "the getter made the area");
        assert_eq!(
            crate::object::prototype_chain::object_static_prototype(area),
            Some(crate::value::js_nanbox_pointer(proto as i64).to_bits()),
            "the area inherits Storage.prototype"
        );
    }
}

/// A lazy interface child links to its parent in its own birth: the same two
/// links the population makes for an eager child.
#[test]
fn lazy_interface_children_link_to_their_parent_in_their_birth() {
    let _lock = crate::gc::global_side_table_test_lock();
    js_get_global_this();
    for (child, parent) in [
        ("CustomEvent", "Event"),
        ("MessageEvent", "Event"),
        ("BroadcastChannel", "EventTarget"),
    ] {
        let child_id = crate::object::BuiltinConstructorIdentity::for_name(child).unwrap();
        let parent_id = crate::object::BuiltinConstructorIdentity::for_name(parent).unwrap();
        assert_eq!(crate::array::intrinsic_prototype_addr_if_bound(child_id), 0, "{child}");
        let proto = crate::value::js_nanbox_get_pointer(builtin_prototype_value(child)) as usize;
        let parent_proto = crate::array::intrinsic_prototype_addr_if_bound(parent_id);
        assert_eq!(
            crate::object::prototype_chain::object_static_prototype(proto),
            Some(crate::value::js_nanbox_pointer(parent_proto as i64).to_bits()),
            "{child}.prototype inherits {parent}.prototype"
        );
        let ctor = crate::array::intrinsic_constructor_addr_if_bound(child_id);
        let parent_ctor = crate::array::intrinsic_constructor_addr_if_bound(parent_id);
        assert_eq!(
            crate::closure::closure_static_prototype(ctor),
            Some(crate::value::js_nanbox_pointer(parent_ctor as i64).to_bits()),
            "{child} inherits the {parent} constructor"
        );
    }
}
