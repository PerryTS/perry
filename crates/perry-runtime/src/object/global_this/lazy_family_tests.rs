//! LAZYPROTO P1: builtin families born on first observation
//! (`BuiltinConstructorDeclaration::lazy`).
use super::*;

fn lazy_declarations() -> Vec<&'static BuiltinConstructorDeclaration> {
    GLOBAL_THIS_BUILTIN_CONSTRUCTORS
        .iter()
        .filter(|declaration| born_by_its_observation(declaration))
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

/// The family's constructor: its globalThis binding, or (a row %Global% does
/// not bind) the realm's [[Intrinsics]] entry.
fn constructor(declaration: &BuiltinConstructorDeclaration) -> f64 {
    if !declaration.global {
        let ctor = crate::array::intrinsic_constructor_addr_if_bound(declaration.identity);
        return crate::value::js_nanbox_pointer(ctor as i64);
    }
    js_get_global_this_builtin_value(declaration.name.as_ptr(), declaration.name.len())
}

/// Fixture 4: realm setup allocates and binds a lazy family's constructor
/// and births nothing else; the first ask for its prototype births it.
#[test]
fn realm_population_does_not_birth_lazy_families() {
    let _lock = crate::gc::global_side_table_test_lock();
    js_get_global_this();
    let lazy = lazy_declarations();
    // Object is the root family: born at realm setup, exactly (320-reply).
    let object = crate::object::BuiltinConstructorIdentity::Object;
    assert!(!object.declaration().lazy, "Object is not declared lazy");
    assert!(!born_by_its_observation(object.declaration()));
    assert_ne!(
        crate::array::intrinsic_constructor_addr_if_bound(object),
        0,
        "Object: setup binds the constructor"
    );
    assert_ne!(
        crate::array::intrinsic_prototype_addr_if_bound(object),
        0,
        "Object is born at realm setup"
    );
    // Function is the second root family: born at realm setup, exactly (321-reply).
    let function = crate::object::BuiltinConstructorIdentity::Function;
    assert!(
        !function.declaration().lazy,
        "Function is not declared lazy"
    );
    assert!(!born_by_its_observation(function.declaration()));
    assert_ne!(
        crate::array::intrinsic_constructor_addr_if_bound(function),
        0,
        "Function: setup binds the constructor"
    );
    assert_ne!(
        crate::array::intrinsic_prototype_addr_if_bound(function),
        0,
        "Function is born at realm setup"
    );
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
        let proto = builtin_prototype_value(declaration.realm_name);
        assert!(JSValue::from_bits(proto.to_bits()).is_pointer());
        assert_eq!(
            crate::array::intrinsic_prototype_addr_if_bound(declaration.identity),
            crate::value::js_nanbox_get_pointer(proto) as usize,
            "{}: the first ask births the family",
            declaration.name
        );
    }
}

extern "C" fn declared_static_fixture(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    value
}

/// Fixture 5: every `lazy` declaration can be born late. Its constructor
/// owns `length`, `name`, `prototype` and its declared statics only, in the
/// realm source's own-key order, the realm seal does not name it (the seal
/// would read its prototype at setup), and no setup step reached its
/// prototype (the population test above). Listing a bagless constructor's
/// keys births nothing.
#[test]
fn lazy_declarations_are_bornable_late() {
    let _lock = crate::gc::global_side_table_test_lock();
    // The provider installs the declared statics at the birth (the stdlib's
    // streams install, in miniature).
    crate::external_family::provide_from_for_tests(crate::fn_info!(
        declared_static_fixture, 1; with_declared(1)
    ));
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
        // Audited (P1-AUDIT): every reader of Array.prototype's identity word
        // reads its family cell or is `_if_bound`. WeakMap / WeakSet / WeakRef: their
        // CLASS word is read as a [[Prototype]] only through an instance's
        // layout, and the one instance birth (`js_weakmap_new` /
        // `js_weakset_new` / `js_weakref_new`) observes the family cell first; every other
        // reader compares the word (the `_if_bound` form).
        const IDENTITY_WORD_AUDITED: &[&str] = &[
            "Array",
            "WeakMap",
            "WeakSet",
            "WeakRef",
            "DataView",
            "URL",
            "URLSearchParams",
            "FinalizationRegistry",
        ];
        assert!(
            IDENTITY_WORD_AUDITED.contains(&declaration.name)
                || declaration.prototype_class.is_none()
                    && declaration.prototype_serial.is_none()
                    && declaration.sealed_serial.is_none()
                    && declaration.named_serial.is_none(),
            "{}: its prototype names an identity word",
            declaration.name
        );
        let before = own_names(constructor(declaration));
        assert_eq!(
            crate::array::intrinsic_prototype_addr_if_bound(declaration.identity),
            0,
            "{}: listing its constructor's keys birthed it",
            declaration.name
        );
        let declared: Vec<&str> = ["length", "name", "prototype"]
            .into_iter()
            .chain(declaration.statics.iter().copied())
            .collect();
        assert_eq!(before, declared, "{}", declaration.name);
        for key in declaration.statics {
            let ptr = crate::value::js_nanbox_get_pointer(constructor(declaration)) as usize;
            assert!(
                crate::object::has_own_helpers::closure_own_key_present(ptr, key)
                    && crate::array::intrinsic_prototype_addr_if_bound(declaration.identity) == 0,
                "{}.{key}: presence birthed it",
                declaration.name
            );
        }
        builtin_prototype_value(declaration.name);
        assert_eq!(
            own_names(constructor(declaration)),
            declared,
            "{}: the birth gave the constructor other keys than it declares",
            declaration.name
        );
    }
}

/// A lazy constructor's declared statics are the realm source's: its own-key
/// order there is `length`, `name`, `prototype`, then exactly `statics`.
#[test]
fn lazy_declared_statics_are_the_realm_sources() {
    let realm: serde_json::Value =
        serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/realm-v1.json")))
            .expect("realm-v1.json");
    let declarations = realm["declarations"].as_array().expect("declarations");
    for declaration in lazy_declarations() {
        let source = declarations
            .iter()
            .find(|d| d["kind"] == "constructor" && d["name"] == declaration.realm_name)
            .unwrap_or_else(|| panic!("{}: no realm constructor declaration", declaration.name));
        let order: Vec<&str> = source["own_key_order"]
            .as_array()
            .expect("own_key_order")
            .iter()
            .map(|key| key.as_str().expect("key"))
            .collect();
        let declared: Vec<&str> = ["length", "name", "prototype"]
            .into_iter()
            .chain(declaration.statics.iter().copied())
            .collect();
        assert_eq!(order, declared, "{}: declared statics differ from the realm source", declaration.name);
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
    "ReadableByteStreamController",
    "ReadableStreamBYOBReader",
    "ReadableStreamBYOBRequest",
];

/// RSTREAM A2: every Web Streams readable family, the byte-stream ones
/// included, is an external family: its members are the streams provider's,
/// so no stream code is linked into a program that never reaches one.
#[test]
fn readable_stream_families_are_external() {
    for name in [
        "ReadableStream",
        "ReadableStreamDefaultController",
        "ReadableStreamDefaultReader",
        "ReadableByteStreamController",
        "ReadableStreamBYOBReader",
        "ReadableStreamBYOBRequest",
    ] {
        let declaration = crate::object::BuiltinConstructorIdentity::for_name(name)
            .unwrap_or_else(|| panic!("{name} is not a builtin constructor"))
            .declaration();
        assert!(declaration.lazy && declaration.external, "{name}: lazy external");
    }
}

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

/// node: `performance` is an accessor (get + set, enumerable, configurable);
/// the 7 perf_hooks constructors are data properties (writable,
/// non-enumerable, configurable), identical to the module exports. Setup
/// runs nothing of perf_hooks: EventTarget stays unborn until a perf family
/// is observed, and `Performance` extends EventTarget by the `extends` rule.
#[test]
fn perf_hooks_families_are_born_by_their_first_read() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::perf_hook_slots::install();
    let global_value = js_get_global_this();
    let global = crate::value::js_nanbox_get_pointer(global_value) as usize;
    let event_target = crate::object::BuiltinConstructorIdentity::EventTarget;
    assert_eq!(
        crate::array::intrinsic_prototype_addr_if_bound(event_target),
        0,
        "EventTarget is unborn after setup"
    );
    let accessor = crate::object::get_accessor_descriptor(global, "performance")
        .expect("performance is an accessor");
    assert_ne!(accessor.get, 0, "performance has a getter");
    assert_ne!(accessor.set, 0, "performance has a setter");
    let attrs =
        crate::object::get_property_attrs(global, "performance").expect("performance attrs");
    assert!(
        attrs.enumerable() && attrs.configurable(),
        "performance: e and c"
    );
    assert_eq!(
        crate::array::intrinsic_prototype_addr_if_bound(event_target),
        0,
        "the performance descriptor births nothing"
    );
    // Reading `performance` creates perf_hooks' singleton, whose
    // [[Prototype]] is Performance.prototype: the creation site births the
    // Performance family (and EventTarget, by `extends`).
    let performance_key = crate::string::js_string_from_bytes(b"performance".as_ptr(), 11);
    let performance_value =
        js_object_get_field_by_name_f64(global as *const ObjectHeader, performance_key);
    assert_ne!(
        crate::array::intrinsic_prototype_addr_if_bound(
            crate::object::BuiltinConstructorIdentity::Performance
        ),
        0,
        "reading performance births Performance"
    );
    for name in [
        "Performance",
        "PerformanceEntry",
        "PerformanceMark",
        "PerformanceMeasure",
        "PerformanceObserver",
        "PerformanceObserverEntryList",
        "PerformanceResourceTiming",
    ] {
        let identity = crate::object::BuiltinConstructorIdentity::for_name(name)
            .unwrap_or_else(|| panic!("{name} is a realm family"));
        assert!(identity.declaration().lazy, "{name} is declared lazy");
        assert!(
            crate::object::get_accessor_descriptor(global, name).is_none(),
            "{name} is a data property"
        );
        let attrs = crate::object::get_property_attrs(global, name)
            .unwrap_or_else(|| panic!("{name} attrs"));
        assert!(
            attrs.writable() && !attrs.enumerable() && attrs.configurable(),
            "{name}: w, not e, c"
        );
        let value = js_get_global_this_builtin_value(name.as_ptr(), name.len());
        let ctor = crate::array::intrinsic_constructor_addr(identity);
        assert_eq!(
            value.to_bits(),
            crate::value::js_nanbox_pointer(ctor as i64).to_bits(),
            "{name}: the realm constructor"
        );
        let prototype = builtin_prototype_value(name);
        assert_eq!(
            prototype.to_bits(),
            crate::value::js_nanbox_pointer(crate::array::intrinsic_prototype_addr_if_bound(
                identity
            ) as i64)
            .to_bits(),
            "{name}: born by the first ask for its prototype"
        );
        let export = crate::object::bound_native_callable_export_value("perf_hooks", name);
        assert_eq!(
            export.to_bits(),
            value.to_bits(),
            "require(\"perf_hooks\").{name} === {name}"
        );
    }
    let event_target_proto = crate::array::intrinsic_prototype_addr_if_bound(event_target);
    assert_ne!(event_target_proto, 0, "Performance births its parent");
    let performance = crate::array::intrinsic_prototype_addr(
        crate::object::BuiltinConstructorIdentity::Performance,
    );
    let parent = crate::object::js_object_get_prototype_of(crate::value::js_nanbox_pointer(
        performance as i64,
    ));
    assert_eq!(
        parent.to_bits(),
        crate::value::js_nanbox_pointer(event_target_proto as i64).to_bits(),
        "Performance.prototype extends EventTarget.prototype"
    );
    assert_eq!(
        crate::object::js_object_get_prototype_of(performance_value).to_bits(),
        crate::value::js_nanbox_pointer(performance as i64).to_bits(),
        "performance's [[Prototype]] is Performance.prototype"
    );
    let mark = crate::array::intrinsic_constructor_addr(
        crate::object::BuiltinConstructorIdentity::PerformanceMark,
    );
    let length_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let length = js_object_get_field_by_name_f64(mark as *const ObjectHeader, length_key);
    assert_eq!(
        length.to_bits(),
        1.0f64.to_bits(),
        "PerformanceMark.length === 1"
    );
}

/// %TypedArray% is a declared lazy family without a globalThis binding: one
/// constructor that is the [[Prototype]] of all 12 typed-array constructors
/// (their declared `extends`), born by the first typed-array observation.
#[test]
fn typed_array_parent_is_one_lazy_family_born_by_its_first_observation() {
    let _lock = crate::gc::global_side_table_test_lock();
    js_get_global_this();
    let parent = crate::object::BuiltinConstructorIdentity::TypedArray;
    assert_eq!(
        crate::object::BuiltinConstructorIdentity::for_name("%TypedArray%"),
        Some(parent)
    );
    assert!(
        born_by_its_observation(parent.declaration()),
        "%TypedArray% is declared lazy"
    );
    assert!(
        !parent.declaration().global,
        "%TypedArray% has no globalThis binding"
    );
    assert_eq!(
        js_get_global_this_builtin_value(b"TypedArray".as_ptr(), 10).to_bits(),
        crate::value::TAG_UNDEFINED,
        "globalThis.TypedArray is undefined"
    );
    assert_eq!(
        crate::array::intrinsic_prototype_addr_if_bound(parent),
        0,
        "%TypedArray% born at realm setup"
    );
    let children: Vec<_> = GLOBAL_THIS_BUILTIN_CONSTRUCTORS
        .iter()
        .filter(|d| d.extends == Some(parent))
        .collect();
    assert_eq!(
        children.len(),
        12,
        "the 12 typed-array families extend %TypedArray%"
    );
    // The first observation: Int8Array's family. It births its parent.
    let int8 = crate::object::BuiltinConstructorIdentity::Int8Array;
    crate::array::intrinsic_prototype_addr(int8);
    let parent_ctor = crate::array::intrinsic_constructor_addr_if_bound(parent);
    let parent_proto = crate::array::intrinsic_prototype_addr_if_bound(parent);
    assert_ne!(parent_proto, 0, "observing Int8Array births %TypedArray%");
    assert_ne!(parent_ctor, 0);
    for child in children {
        let proto_of_ctor = crate::object::js_object_get_prototype_of(constructor(child));
        assert_eq!(
            proto_of_ctor.to_bits(),
            crate::value::js_nanbox_pointer(parent_ctor as i64).to_bits(),
            "Object.getPrototypeOf({}) is %TypedArray%",
            child.name
        );
        let child_proto = crate::array::intrinsic_prototype_addr(child.identity);
        let proto_of_proto = crate::object::js_object_get_prototype_of(
            crate::value::js_nanbox_pointer(child_proto as i64),
        );
        assert_eq!(
            proto_of_proto.to_bits(),
            crate::value::js_nanbox_pointer(parent_proto as i64).to_bits(),
            "Object.getPrototypeOf({}.prototype) is %TypedArray%.prototype",
            child.name
        );
    }
    assert_eq!(
        crate::array::intrinsic_constructor_addr_if_bound(parent),
        parent_ctor,
        "one %TypedArray%"
    );
}

#[test]
fn builtin_constructor_realm_slots_are_permanent_birth_data() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::object::js_nm_install_vm();
    let _no_move = crate::gc::GcSuppressScope::new();
    let main = crate::array::thread_realm();
    let global = crate::object::js_new_realm_global();
    let global_ptr = (global.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    let vm = unsafe { super::realm_of(global_ptr) };
    for declaration in GLOBAL_THIS_BUILTIN_CONSTRUCTORS {
        for (realm, expected) in [(main, crate::value::TAG_UNDEFINED), (vm, global.to_bits())] {
            let ctor = super::super::global_this_tables::alloc_builtin_constructor(
                declaration.identity,
                realm,
            );
            assert_eq!(
                crate::closure::js_closure_get_capture_bits(ctor, 1),
                expected
            );
            assert!(std::ptr::eq(
                unsafe { super::super::global_this_tables::builtin_constructor_realm(ctor) },
                realm
            ));
        }
    }
}

#[test]
fn typed_array_parent_direct_construction_throws_its_declared_error() {
    let _lock = crate::gc::global_side_table_test_lock();
    js_get_global_this();
    let child = crate::object::BuiltinConstructorIdentity::Int8Array;
    let parent = super::super::js_object_get_prototype_of(constructor(child.declaration()));
    let thrown = crate::exception::catch_js_throw(|| unsafe {
        super::super::js_new_function_construct(parent, std::ptr::null(), 0)
    })
    .expect_err("%TypedArray% is abstract");
    let key = crate::string::js_string_from_bytes(b"message".as_ptr(), 7);
    let message = super::super::js_object_get_field_by_name(
        (thrown.to_bits() & crate::value::POINTER_MASK) as *const ObjectHeader,
        key,
    );
    let message =
        super::super::property_ops::set::key_to_rust_string(f64::from_bits(message.bits()))
            .unwrap();
    assert_eq!(
        message,
        "Abstract class TypedArray not directly constructable"
    );
}

#[test]
fn default_parent_birth_does_not_materialize_main_global() {
    assert!(!crate::object::global_this_is_materialized());
    let prototype =
        crate::array::intrinsic_prototype_addr(crate::object::BuiltinConstructorIdentity::WeakMap);
    assert_ne!(prototype, 0);
    assert!(!crate::object::global_this_is_materialized());
}
