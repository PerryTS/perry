//! The two-phase realm (`perry_abi::realm_phase`): phase A alone, then the
//! phase-B driver, against a realm populated in one pass.
use super::*;
use crate::codegen_abi::{
    REALM_CORE_GLOBALS, REALM_REST_GLOBALS, REALM_UNBOUND_POISON, REALM_VALUE_GLOBALS,
};

const UNDEFINED: u64 = crate::value::TAG_UNDEFINED;

fn global_ptr(value: f64) -> *mut ObjectHeader {
    crate::value::js_nanbox_get_pointer(value) as *mut ObjectHeader
}

/// The word in the declared slot of `name` of the realm global `global`.
fn slot_word(global: *mut ObjectHeader, name: &str) -> u64 {
    let slot = crate::object::runtime_layouts::builtin_global_slot(name)
        .unwrap_or_else(|| panic!("{name} is a declared global key"));
    // SAFETY: the live realm global, born on its static layout (inline slots).
    unsafe { *crate::meta_slots_of!(crate::meta_obj_runtime_born!(global)).add(slot) }
}

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

fn layout_word(global: *mut ObjectHeader) -> u32 {
    // SAFETY: the live realm global.
    unsafe { (*global).parent_class_id }
}

/// What a realm populated in one pass (main's behaviour) looks like: the
/// global's own names, its layout word, and which declared slots stay
/// `undefined` in this build (a hook that is not installed).
struct OnePass {
    names: Vec<String>,
    word: u32,
    undefined: Vec<&'static str>,
}

fn one_pass() -> OnePass {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| {
            let value = js_get_global_this();
            let global = global_ptr(value);
            assert!(fetch_globals::realm_is_complete());
            OnePass {
                names: own_names(value),
                word: layout_word(global),
                undefined: REALM_CORE_GLOBALS
                    .iter()
                    .chain(REALM_REST_GLOBALS)
                    .copied()
                    .filter(|name| slot_word(global, name) == UNDEFINED)
                    .collect(),
            }
        })
        .unwrap()
        .join()
        .unwrap()
}

fn on_core_realm<R: Send + 'static>(run: impl FnOnce(f64) -> R + Send + 'static) -> R {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| run(fetch_globals::core_realm_for_test()))
        .unwrap()
        .join()
        .unwrap()
}

/// CORE, REST and VALUE partition the keys the global's layout declares (the
/// private `[[RegExpPrototype]]` slot aside): no declared name is without a
/// phase, and no listed name is undeclared.
#[test]
fn phase_lists_partition_the_declared_keys() {
    let _lock = crate::gc::global_side_table_test_lock();
    std::thread::spawn(|| {
        js_get_global_this();
        let lists = [REALM_CORE_GLOBALS, REALM_REST_GLOBALS, REALM_VALUE_GLOBALS];
        let mut slots = std::collections::BTreeMap::new();
        for list in lists {
            for name in list {
                let slot = crate::object::runtime_layouts::builtin_global_slot(name)
                    .unwrap_or_else(|| panic!("{name} is listed but not declared"));
                assert!(
                    slots.insert(slot, *name).is_none(),
                    "{name} is in two phase lists"
                );
            }
        }
        assert_eq!(
            slots.len() + 1,
            crate::object::runtime_layouts::GLOBAL_OBJECT::SLOT_COUNT,
            "every declared key but the private slot has a phase"
        );
        for name in REALM_VALUE_GLOBALS {
            assert!(!crate::codegen_abi::realm_core_global(name));
            assert!(!crate::codegen_abi::realm_rest_global(name));
        }
    })
    .join()
    .unwrap();
}

/// Phase A alone binds every core name and no phase-B name or intrinsic (a
/// core family that needed a phase-B family would be a table error); the
/// global's keys, their order and its layout word are those of a realm
/// populated in one pass before phase B and after it; phase B binds the rest
/// and its intrinsics are the objects the global's slots name.
#[test]
fn core_then_rest_equals_one_pass() {
    let _lock = crate::gc::global_side_table_test_lock();
    let reference = one_pass();
    on_core_realm(move |value| {
        let global = global_ptr(value);
        assert!(global_this_is_materialized());
        assert!(!fetch_globals::realm_is_complete());
        assert_eq!(layout_word(global), reference.word, "before phase B");
        assert_eq!(own_names(value), reference.names, "before phase B");
        for name in REALM_REST_GLOBALS {
            assert_eq!(slot_word(global, name), REALM_UNBOUND_POISON, "{name}");
            if let Some(identity) = BuiltinConstructorIdentity::for_name(name) {
                assert_eq!(
                    crate::array::intrinsic_constructor_addr_if_bound(identity),
                    0,
                    "phase A bound the phase-B intrinsic {name}"
                );
                assert_eq!(crate::array::intrinsic_prototype_addr_if_bound(identity), 0);
            }
        }
        for name in REALM_CORE_GLOBALS {
            let word = slot_word(global, name);
            assert_ne!(word, REALM_UNBOUND_POISON, "{name}");
            assert_eq!(
                word == UNDEFINED,
                reference.undefined.contains(name),
                "{name} after phase A"
            );
        }
        // The core entry hands out the same object and starts nothing.
        assert_eq!(js_get_global_core().to_bits(), value.to_bits());
        assert!(!fetch_globals::realm_is_complete());
        // The full entry runs phase B.
        assert_eq!(js_get_global_this().to_bits(), value.to_bits());
        assert!(fetch_globals::realm_is_complete());
        let global = global_ptr(js_get_global_this());
        assert_eq!(layout_word(global), reference.word, "after phase B");
        assert_eq!(
            own_names(js_get_global_this()),
            reference.names,
            "after phase B"
        );
        for name in REALM_REST_GLOBALS {
            let word = slot_word(global, name);
            assert_ne!(word, REALM_UNBOUND_POISON, "{name}");
            assert_eq!(
                word == UNDEFINED,
                reference.undefined.contains(name),
                "{name} after phase B"
            );
            if let Some(identity) = BuiltinConstructorIdentity::for_name(name) {
                let ctor = crate::array::intrinsic_constructor_addr_if_bound(identity);
                assert_ne!(ctor, 0, "{name}");
                assert_eq!(
                    crate::value::js_nanbox_get_pointer(f64::from_bits(word)) as usize,
                    ctor,
                    "{name}: the global binds the realm's one constructor"
                );
                assert_eq!(
                    crate::value::js_nanbox_get_pointer(builtin_constructor_value(name)) as usize,
                    ctor
                );
            }
        }
    });
}

/// A runtime reader of a phase-A intrinsic leaves phase B alone; a reader of
/// a phase-B intrinsic (`realm_intrinsic_addr`, the internal births' one
/// materialize point) runs it, and gets the object the global then names.
#[test]
fn internal_readers_run_the_phase_they_need() {
    let _lock = crate::gc::global_side_table_test_lock();
    on_core_realm(|_| {
        for name in ["Map", "Array", "Promise", "Error", "Uint8Array"] {
            assert!(JSValue::from_bits(builtin_prototype_value(name).to_bits()).is_pointer());
            assert!(JSValue::from_bits(builtin_constructor_value(name).to_bits()).is_pointer());
            let by_name = js_get_global_this_builtin_value(name.as_ptr(), name.len());
            assert_eq!(by_name.to_bits(), builtin_constructor_value(name).to_bits());
        }
        // A lazy phase-A family is born without phase B.
        assert!(JSValue::from_bits(builtin_prototype_value("TextEncoder").to_bits()).is_pointer());
        assert!(
            !fetch_globals::realm_is_complete(),
            "a phase-A read ran phase B"
        );
        let proto = builtin_prototype_value("WeakMap");
        assert!(fetch_globals::realm_is_complete());
        assert!(JSValue::from_bits(proto.to_bits()).is_pointer());
        let ctor = js_get_global_this_builtin_value("WeakMap".as_ptr(), 7);
        assert_eq!(
            ctor.to_bits(),
            builtin_constructor_value("WeakMap").to_bits()
        );
        let via_ctor = crate::closure::closure_get_dynamic_prop(
            crate::value::js_nanbox_get_pointer(ctor) as usize,
            "prototype",
        );
        assert_eq!(via_ctor.to_bits(), proto.to_bits());
    });
    on_core_realm(|_| {
        let ctor = builtin_constructor_value("WeakRef");
        assert!(fetch_globals::realm_is_complete());
        assert!(JSValue::from_bits(ctor.to_bits()).is_pointer());
    });
    on_core_realm(|_| {
        // A by-name read of a phase-B name, and of an undeclared one.
        let intl = js_get_global_this_builtin_value("Intl".as_ptr(), 4);
        assert!(fetch_globals::realm_is_complete());
        assert_ne!(intl.to_bits(), REALM_UNBOUND_POISON);
    });
    on_core_realm(|_| {
        let missing = js_get_global_this_builtin_value("notAGlobal".as_ptr(), 10);
        assert!(fetch_globals::realm_is_complete());
        assert_eq!(missing.to_bits(), UNDEFINED);
    });
}

/// The phase-B families the realm seals carry their replace-lock from phase
/// B on (`seal_realm_family_of`), and the namespaces their seal.
#[test]
fn phase_b_families_are_sealed_when_bound() {
    let _lock = crate::gc::global_side_table_test_lock();
    on_core_realm(|_| {
        js_get_global_this();
        for name in ["WeakMap", "WeakSet"] {
            let proto = crate::value::js_nanbox_get_pointer(builtin_prototype_value(name))
                as *mut ObjectHeader;
            // SAFETY: the live intrinsic prototype.
            let sealed = unsafe {
                let meta = crate::meta_read!(crate::meta_obj_runtime_born!(proto));
                !meta.is_null()
                    && (*meta).flags & crate::object::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC
                        != 0
            };
            assert!(sealed, "{name}.prototype is sealed");
        }
    });
}
