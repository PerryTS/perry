//! Step 6 slice 3b-i: `%Object.prototype%`, `%Function.prototype%` and
//! `%Array.prototype%`'s holder are static layouts, and the probe answers
//! each hop through them from the layout. A program's write to one of them
//! must be read back (or declined), never answered from the pristine layout.
use super::*;
use crate::codegen_abi::LayoutWord;
use crate::value::{TAG_HOLE, TAG_UNDEFINED};

fn on_a_fresh_thread(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(body)
        .expect("spawn builtin-layout probe test thread")
        .join()
        .expect("builtin-layout probe test thread panicked");
}

unsafe fn atom(key: &str) -> u64 {
    let hash = crate::object::key_bytes_hash(key.as_ptr(), key.len());
    let a = crate::string::js_string_pool_atom(key.as_ptr(), key.len() as u32, hash, 0);
    STRING_TAG | a as u64
}

/// The probe's answer for `key` on the heap object `obj`.
unsafe fn probe(obj: usize, key: &str) -> u64 {
    let bits = atom(key);
    let hash = crate::string::property_atom_hash((bits & POINTER_MASK) as *const _);
    perry_probe_get(
        test_dir(),
        obj.wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
        bits,
        hash,
    )
    .to_bits()
}

/// `Reflect.get(obj, key)`.
unsafe fn get(obj: usize, key: &str) -> u64 {
    let boxed = f64::from_bits(POINTER_TAG | obj as u64);
    crate::proxy::js_reflect_get(boxed, f64::from_bits(atom(key)), boxed).to_bits()
}

/// The probe either declines or agrees with the complete read.
unsafe fn agrees(obj: usize, key: &str) -> u64 {
    let p = probe(obj, key);
    assert!(
        p == TAG_HOLE || p == get(obj, key),
        "probe of {key} answered {p:#x}, the complete read {:#x}",
        get(obj, key)
    );
    p
}

unsafe fn global(name: &str) -> usize {
    let v = crate::object::js_get_global_this_builtin_value(name.as_ptr(), name.len());
    (v.to_bits() & POINTER_MASK) as usize
}

unsafe fn realm() -> (usize, usize, usize) {
    let _ = crate::object::js_get_global_this();
    (
        crate::array::object_prototype_addr(),
        crate::array::function_prototype_addr(),
        crate::array::array_prototype_addr(),
    )
}

unsafe fn set(obj: usize, key: &str, value: f64) {
    crate::object::js_object_set_field_by_name(
        obj as *mut ObjectHeader,
        (atom(key) & POINTER_MASK) as *const crate::StringHeader,
        value,
    );
}

#[test]
fn builtin_prototypes_are_born_on_their_static_layouts() {
    on_a_fresh_thread(|| unsafe {
        let (op, fp, ap) = realm();
        let holder = crate::array::array_property_bag(ap as *const crate::array::ArrayHeader);
        for (obj, desc) in [
            (
                op as *const ObjectHeader,
                crate::object::runtime_layouts::OBJECT_PROTOTYPE::desc(),
            ),
            (
                fp as *const ObjectHeader,
                crate::object::runtime_layouts::FUNCTION_PROTOTYPE::desc(),
            ),
            (
                holder as *const ObjectHeader,
                crate::object::runtime_layouts::ARRAY_PROTOTYPE_NAMED::desc(),
            ),
        ] {
            let r = record_of(test_dir(), LayoutWord((*obj).parent_class_id));
            assert!(!r.is_null());
            // The runtime's own population is no program write: no shadow.
            assert_eq!(
                (*obj).parent_class_id & crate::object::overflow::SHADOW_BIT,
                0
            );
            let d = &*desc;
            let declared = record_of(test_dir(), LayoutWord(d.layout_id));
            assert!(!declared.is_null(), "the layout is registered");
            // The layout's own key list, every key an atom, every slot filled.
            assert_eq!((*r).keys, (*declared).keys, "the declared key list");
            assert_eq!((*r).logical_key_count, d.key_count);
            assert_eq!((*r).object_kind(), (*declared).object_kind());
            let pw = probe_word(r);
            assert!(pw & PROBE_ATOMS != 0 && pw & PROBE_DECLINE == 0);
            for i in 0..d.key_count {
                let slot = (*d.keys.add(i as usize)).slot as usize;
                let v = *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + slot * 8)
                    as *const u64);
                assert_ne!(v, TAG_HOLE, "declared slot {slot} is filled");
            }
        }
        // The Annex B helpers are own properties of %Object.prototype%.
        assert_ne!(get(op, "__lookupGetter__"), TAG_UNDEFINED);
    });
}

#[test]
fn closure_and_array_reads_continue_through_the_builtin_prototypes() {
    on_a_fresh_thread(|| unsafe {
        let (op, _fp, ap) = realm();
        let object_ctor = global("Object");
        let array_ctor = global("Array");
        let arr = crate::array::js_array_alloc(0) as usize;
        // Absent through %Function.prototype% and %Object.prototype%.
        assert_eq!(agrees(object_ctor, "isBuffer"), TAG_UNDEFINED);
        // `Array` carries an accessor (`Array[Symbol.species]`): its bag
        // proves the key absent and the read continues the same way.
        assert_eq!(agrees(array_ctor, "isBuffer"), TAG_UNDEFINED);
        // Inherited through %Function.prototype%.
        assert_ne!(agrees(object_ctor, "hasOwnProperty"), TAG_UNDEFINED);
        // An own key is read; an inherited accessor (`caller`) is declined.
        agrees(object_ctor, "name");
        assert_eq!(probe(object_ctor, "caller"), TAG_HOLE);
        // An array: %Array.prototype%'s holder, then %Object.prototype%.
        assert_eq!(agrees(arr, "constructor"), POINTER_TAG | array_ctor as u64);
        assert_eq!(agrees(arr, "isBuffer"), TAG_UNDEFINED);
        assert_eq!(agrees(arr, "hasOwnProperty"), get(op, "hasOwnProperty"));
        assert_eq!(agrees(ap, "isBuffer"), TAG_UNDEFINED);
        // A plain object ends at %Object.prototype%, whose miss is absence.
        let o = crate::object::js_object_alloc(0, 1) as usize;
        assert_eq!(agrees(o, "isBuffer"), TAG_UNDEFINED);
        assert_eq!(agrees(o, "__lookupSetter__"), get(op, "__lookupSetter__"));
    });
}

#[test]
fn writes_to_the_builtin_prototypes_are_read_back() {
    on_a_fresh_thread(|| unsafe {
        let (op, fp, ap) = realm();
        let object_ctor = global("Object");
        let arr = crate::array::js_array_alloc(0) as usize;
        let o = crate::object::js_object_alloc(0, 1) as usize;
        for (obj, key) in [(object_ctor, "isBuffer"), (arr, "extra"), (o, "extra")] {
            assert_eq!(agrees(obj, key), TAG_UNDEFINED, "pristine: {key} is absent");
        }
        crate::object::own_override::as_builtin_definition(|| set(fp, "isBuffer", 7.0));
        assert_eq!(get(object_ctor, "isBuffer"), 7.0f64.to_bits());
        agrees(object_ctor, "isBuffer");
        crate::object::own_override::as_builtin_definition(|| set(op, "extra", 8.0));
        for obj in [arr, o, object_ctor] {
            assert_eq!(get(obj, "extra"), 8.0f64.to_bits());
            agrees(obj, "extra");
        }
        crate::array::named_props::array_named_property_set(
            ap as *mut crate::array::ArrayHeader,
            (atom("mapAdded") & POINTER_MASK) as *const crate::StringHeader,
            9.0,
        );
        assert_eq!(get(arr, "mapAdded"), 9.0f64.to_bits());
        agrees(arr, "mapAdded");
        // DEC 250 Q2: the runtime's own definitions above are read back; a
        // program add on either add-locked prototype is a TypeError that
        // changes nothing: the layout word and every read stay as they were.
        for p in [op, fp] {
            let word = (*(p as *const ObjectHeader)).parent_class_id;
            crate::exception::catch_js_throw(|| set(p, "programAdded", 10.0))
                .expect_err("a program add on an add-locked prototype is refused");
            assert_eq!(
                (*(p as *const ObjectHeader)).parent_class_id,
                word,
                "a refused add keeps the word"
            );
            for obj in [arr, o, object_ctor] {
                assert_eq!(
                    get(obj, "programAdded"),
                    TAG_UNDEFINED,
                    "a refused add is not read back"
                );
                agrees(obj, "programAdded");
            }
        }
    });
}

#[test]
fn a_deleted_and_readded_builtin_is_read_back() {
    on_a_fresh_thread(|| unsafe {
        let (op, fp, _ap) = realm();
        let object_ctor = global("Object");
        let arr = crate::array::js_array_alloc(0) as usize;
        let o = crate::object::js_object_alloc(0, 1) as usize;
        for (proto, key) in [
            (op, "toLocaleString"),
            (fp, "apply"),
            (op, "propertyIsEnumerable"),
        ] {
            crate::object::own_override::as_builtin_definition(|| {
                crate::object::js_object_delete_field(
                    proto as *mut ObjectHeader,
                    (atom(key) & POINTER_MASK) as *const crate::StringHeader,
                )
            });
            for obj in [o, arr, object_ctor] {
                agrees(obj, key);
            }
            crate::object::own_override::as_builtin_definition(|| set(proto, key, 11.0));
            eprintln!(
                "readded {key}: overflow={}",
                !crate::object::overflow::table(proto as *const ObjectHeader).is_null()
            );
            for obj in [o, arr, object_ctor] {
                agrees(obj, key);
            }
            assert_eq!(
                get(o, key),
                if proto == op {
                    11.0f64.to_bits()
                } else {
                    get(o, key)
                }
            );
        }
        // After a delete the overflow is the order authority: a new key
        // lands there, off the layout, and the shadow bit sends the probe
        // to it. Only the runtime's own definition adds such a key: the
        // program's add through the overflow is refused (DEC 250 Q2).
        for (proto, key) in [(op, "addedAfterDelete"), (fp, "addedAfterDelete2")] {
            crate::exception::catch_js_throw(|| set(proto, key, 13.0))
                .expect_err("a program add through the overflow is refused");
            for obj in [o, arr, object_ctor] {
                assert_eq!(
                    get(obj, key),
                    TAG_UNDEFINED,
                    "a refused add is not read back"
                );
                agrees(obj, key);
            }
            crate::object::own_override::as_builtin_definition(|| set(proto, key, 12.0));
            for obj in [o, arr, object_ctor] {
                if proto == op || obj == object_ctor {
                    assert_eq!(get(obj, key), 12.0f64.to_bits());
                }
                agrees(obj, key);
            }
        }
    });
}

unsafe fn data_descriptor(value: u64, writable: bool, enumerable: bool, configurable: bool) -> f64 {
    let bag = crate::object::js_object_alloc(0, 4) as usize;
    let b = |v: bool| {
        f64::from_bits(if v {
            crate::value::TAG_TRUE
        } else {
            crate::value::TAG_FALSE
        })
    };
    set(bag, "value", f64::from_bits(value));
    set(bag, "writable", b(writable));
    set(bag, "enumerable", b(enumerable));
    set(bag, "configurable", b(configurable));
    f64::from_bits(POINTER_TAG | bag as u64)
}

/// Population and no-op redefinitions (the same value and attributes) leave
/// each builtin prototype on its declared layout: being a prototype is a fact
/// of the layout every birth on it is marked with, not a private lineage.
#[test]
fn builtin_prototypes_stay_on_their_declared_layout_ids() {
    on_a_fresh_thread(|| unsafe {
        let (op, fp, ap) = realm();
        let holder = crate::array::array_property_bag(ap as *const crate::array::ArrayHeader);
        let cases = [
            (
                op,
                op as *const ObjectHeader,
                crate::object::runtime_layouts::OBJECT_PROTOTYPE::desc(),
                "toString",
            ),
            (
                fp,
                fp as *const ObjectHeader,
                crate::object::runtime_layouts::FUNCTION_PROTOTYPE::desc(),
                "call",
            ),
            (
                ap,
                holder as *const ObjectHeader,
                crate::object::runtime_layouts::ARRAY_PROTOTYPE_NAMED::desc(),
                "map",
            ),
        ];
        for (_, obj, desc, _) in cases {
            assert_eq!(
                crate::object::layout_desc::layout_id_of_word((*obj).parent_class_id),
                (*desc).layout_id,
                "family {:#x} is on its declared layout after population",
                (*desc).family
            );
        }
        for (target, obj, desc, key) in cases {
            let boxed = f64::from_bits(POINTER_TAG | target as u64);
            let value = get(target, key);
            crate::object::own_override::as_builtin_definition(|| {
                crate::object::object_ops::js_object_define_property(
                    boxed,
                    f64::from_bits(atom(key)),
                    data_descriptor(value, true, false, true),
                )
            });
            assert_eq!(get(target, key), value);
            assert_eq!(
                crate::object::layout_desc::layout_id_of_word((*obj).parent_class_id),
                (*desc).layout_id,
                "a no-op redefinition of {key} keeps family {:#x} on its layout",
                (*desc).family
            );
        }
    });
}

/// `URL.prototype`, `Timeout.prototype` and `Immediate.prototype` are sealed
/// runtime prototypes: a compiled plain store (the named-store fast path)
/// must not be admitted on them, so every write reaches the funnel that sets
/// the prototype's shadow bit, and the probe from an object born on their
/// layouts reads the write back (or declines).
#[test]
fn sealed_runtime_prototypes_take_the_store_funnel() {
    on_a_fresh_thread(|| unsafe {
        let _ = realm();
        let url = (get(global("URL"), "prototype") & POINTER_MASK) as usize;
        let [timeout, immediate] = crate::timer::timer_prototypes_for_tests();
        for (name, proto, desc) in [
            (
                "URL",
                url as *mut ObjectHeader,
                crate::object::runtime_layouts::URL_COMPONENTS::desc(),
            ),
            (
                "Timeout",
                timeout,
                crate::object::runtime_layouts::TIMEOUT::desc(),
            ),
            (
                "Immediate",
                immediate,
                crate::object::runtime_layouts::IMMEDIATE::desc(),
            ),
        ] {
            assert!(!proto.is_null());
            let word = (*proto).parent_class_id;
            assert!(
                !crate::object::layout_desc::word_is_shadowed(word),
                "{name}: pristine"
            );
            let kind = crate::object::shapes::shape_object_kind_for_word(LayoutWord(
                crate::object::layout_desc::layout_id_of_word(word),
            ));
            eprintln!("{name}.prototype kind {kind:?}");
            assert_ne!(
                kind,
                Some(crate::object::shapes::ShapeObjectKind::Ordinary),
                "{name}.prototype admits no plain store"
            );
            let born = crate::object::layout_desc::perry_alloc_layout(desc) as usize;
            let key = "addedByProgram";
            let stored = crate::object::shape_store::try_store(
                f64::from_bits(POINTER_TAG | proto as u64),
                f64::from_bits(atom(key)),
                13.0,
            );
            if stored.is_none() {
                set(proto as usize, key, 13.0);
            }
            assert!(
                crate::object::layout_desc::word_is_shadowed((*proto).parent_class_id),
                "{name}: a store sets the shadow bit"
            );
            assert_eq!(get(proto as usize, key), 13.0f64.to_bits());
            let p = probe(born, key);
            assert!(
                p == TAG_HOLE || p == 13.0f64.to_bits(),
                "{name}: probe answered {p:#x}"
            );
        }
    });
}

#[test]
fn functions_answer_their_own_and_inherited_members_through_the_layouts() {
    on_a_fresh_thread(|| unsafe {
        let (op, fp, _ap) = realm();
        // (`Object` owns non-spec `toString` / `valueOf` statics; `Function`
        // owns only `length`, `name` and `prototype`.)
        let ctor = global("Function");
        // Own `length`, the function layout's own data: answered from the
        // body of a function with no own-property bag, not from
        // %Function.prototype%'s `length` (0).
        let f = crate::closure::js_closure_alloc(
            crate::fn_info!(two_parameters, 2; with_declared(2)),
            0,
        ) as usize;
        assert!((*(f as *const crate::closure::ClosureHeader))
            .props
            .is_null());
        assert_eq!(probe(f, "length"), 2.0f64.to_bits());
        assert_eq!(get(f, "length"), 2.0f64.to_bits());
        assert_eq!(agrees(ctor, "length"), get(ctor, "length"));
        assert_ne!(get(ctor, "length"), get(fp, "length"));
        let name = agrees(ctor, "name");
        assert!(name == TAG_HOLE || name != get(fp, "name"));
        // %Function.prototype%'s own members.
        for k in ["constructor", "apply", "bind", "call", "toString"] {
            let v = agrees(ctor, k);
            assert_ne!(v, TAG_HOLE, "{k} is answered from %Function.prototype%");
            assert_eq!(v, get(fp, k), "{k}");
        }
        // Inherited from %Object.prototype%: %Function.prototype% owns no copy.
        for k in [
            "valueOf",
            "toLocaleString",
            "isPrototypeOf",
            "hasOwnProperty",
        ] {
            assert_eq!(agrees(ctor, k), get(op, k), "{k}");
            assert_eq!(probe(fp, k), get(op, k), "{k} on %Function.prototype%");
        }
        // A write to %Function.prototype% is read back through every function,
        // and an own write on the function shadows it.
        crate::object::own_override::as_builtin_definition(|| set(fp, "addedToFunctions", 7.0));
        assert_eq!(get(ctor, "addedToFunctions"), 7.0f64.to_bits());
        agrees(ctor, "addedToFunctions");
        crate::object::own_override::as_builtin_definition(|| set(op, "addedToObjects", 8.0));
        assert_eq!(get(ctor, "addedToObjects"), 8.0f64.to_bits());
        agrees(ctor, "addedToObjects");
        assert_eq!(agrees(op, "addedToObjects"), get(op, "addedToObjects"));
    });
}

/// #12720: a function on FunctionDictionary only because its bag holds an
/// accessor answers own data and absence from its bag and continues at
/// `%Function.prototype%`; an accessor hit, a recorded [[Prototype]] and a
/// deleted intrinsic still decline into the complete read.
#[test]
fn a_function_whose_bag_holds_an_accessor_reads_through_its_bag() {
    on_a_fresh_thread(|| unsafe {
        let (op, fp, _ap) = realm();
        let array_ctor = global("Array");
        let dict = crate::closure::shape::function_dictionary_shape();
        assert_eq!(
            (*(array_ctor as *const crate::closure::ClosureHeader)).shape_id,
            dict,
            "Array[Symbol.species] puts Array on FunctionDictionary"
        );
        assert_eq!(probe(array_ctor, "isBuffer"), TAG_UNDEFINED);
        assert_eq!(get(array_ctor, "isBuffer"), TAG_UNDEFINED);
        let is_array = agrees(array_ctor, "isArray");
        assert_ne!(is_array, TAG_HOLE, "an own static is read from the bag");
        assert_eq!(
            agrees(array_ctor, "hasOwnProperty"),
            get(op, "hasOwnProperty")
        );
        assert_eq!(agrees(array_ctor, "bind"), get(fp, "bind"));

        let new_fn = || {
            crate::closure::js_closure_alloc(
                crate::fn_info!(two_parameters, 2; with_declared(2)),
                0,
            ) as usize
        };
        let with_accessor = |f: usize| {
            crate::object::descriptor_state::set_accessor_descriptor(
                f,
                "x".to_string(),
                crate::object::descriptor_state::AccessorDescriptor { get: 0, set: 0 },
            );
            assert_eq!(
                (*(f as *const crate::closure::ClosureHeader)).shape_id,
                dict
            );
        };
        // A user function with an own accessor and an own data property.
        let f = new_fn();
        crate::closure::closure_set_dynamic_prop(f, "d", 3.0);
        with_accessor(f);
        assert_eq!(probe(f, "missing"), TAG_UNDEFINED);
        assert_eq!(get(f, "missing"), TAG_UNDEFINED);
        assert_eq!(agrees(f, "d"), 3.0f64.to_bits());
        assert_eq!(probe(f, "x"), TAG_HOLE, "an accessor hit declines");
        assert_eq!(agrees(f, "call"), get(fp, "call"));
        // The intrinsic keys: a bag hit, or the complete read synthesizes them.
        for k in ["name", "length", "prototype"] {
            agrees(f, k);
        }

        // A recorded [[Prototype]] lives in the state record: decline.
        let g = new_fn();
        with_accessor(g);
        let proto = crate::object::js_object_alloc(0, 1) as usize;
        set(proto, "missing", 5.0);
        crate::closure::closure_set_static_prototype(
            g,
            crate::value::js_nanbox_pointer(proto as i64).to_bits(),
        );
        assert_eq!(probe(g, "missing"), TAG_HOLE);
        assert_eq!(get(g, "missing"), 5.0f64.to_bits());

        // A deleted intrinsic leaves a state record: decline.
        let h = new_fn();
        with_accessor(h);
        crate::closure::closure_mark_key_deleted(h, "name");
        assert_eq!(probe(h, "missing"), TAG_HOLE);
    });
}

extern "C" fn two_parameters(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _a: f64,
    _b: f64,
) -> f64 {
    0.0
}

#[test]
fn array_intrinsic_length_precedes_named_bag_and_prototypes() {
    on_a_fresh_thread(|| unsafe {
        let _ = realm();
        let scope = crate::gc::RuntimeHandleScope::new();
        let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc(4));
        let key = scope.root_nanbox_f64(f64::from_bits(atom("length")));
        for length in 0..4 {
            let arr = array.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            assert_eq!(agrees(arr as usize, "length"), (length as f64).to_bits());
            // A freshly allocated non-atom key takes the same intrinsic read.
            let dynamic = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
            let dynamic = crate::value::js_nanbox_string(dynamic as i64);
            let arr = array.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            assert_eq!(
                perry_probe_get_keyed(test_dir(), POINTER_TAG | arr as u64, dynamic.to_bits()),
                length as f64,
            );
            let next = crate::array::js_array_push_f64(arr, 10.0);
            array.set_raw_mut_ptr(next);
        }
        let proxy = crate::proxy_hooks::proxy_new(
            crate::value::js_nanbox_pointer(
                array.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64
            ),
            crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64),
        );
        assert_eq!(
            perry_probe_get_keyed(test_dir(), proxy.to_bits(), key.get_nanbox_f64().to_bits())
                .to_bits(),
            TAG_HOLE,
        );
    });
}

/// A fresh ordinary object whose [[Prototype]] is `proto` (an object
/// address, or 0 for null).
unsafe fn create(proto: usize) -> usize {
    let p = if proto == 0 {
        f64::from_bits(crate::value::TAG_NULL)
    } else {
        f64::from_bits(POINTER_TAG | proto as u64)
    };
    (crate::object::js_object_create(p).to_bits() & POINTER_MASK) as usize
}

/// The probe's own miss on a literal of at most eight keys, read as absent
/// through `%Object.prototype%`.
#[test]
fn own_miss_on_a_small_literal_reads_absent() {
    on_a_fresh_thread(|| unsafe {
        let _ = realm();
        let o = crate::object::js_object_alloc(0, 4) as usize;
        for (k, v) in [("a", 1.0), ("b", 2.0), ("c", 3.0), ("d", 4.0)] {
            set(o, k, v);
        }
        assert_eq!(probe(o, "zz"), TAG_UNDEFINED);
        assert_eq!(probe(o, "c"), 3.0f64.to_bits());
        assert_eq!(agrees(o, "toString"), get(o, "toString"));
    });
}

/// `Object.create(P)`: the own miss answers P's value.
#[test]
fn own_miss_reads_the_prototype_value() {
    on_a_fresh_thread(|| unsafe {
        let _ = realm();
        let p = crate::object::js_object_alloc(0, 2) as usize;
        set(p, "inh", 7.0);
        set(p, "other", 8.0);
        let o = create(p);
        set(o, "a", 1.0);
        assert_eq!(probe(o, "inh"), 7.0f64.to_bits());
        assert_eq!(probe(o, "other"), 8.0f64.to_bits());
        assert_eq!(probe(o, "zz"), TAG_UNDEFINED);
    });
}

/// A deleted prototype key: the read continues to the next holder.
#[test]
fn own_miss_past_a_deleted_prototype_key_reads_the_next_holder() {
    on_a_fresh_thread(|| unsafe {
        let _ = realm();
        let g = crate::object::js_object_alloc(0, 1) as usize;
        set(g, "k", 1.0);
        let p = create(g);
        set(p, "k", 2.0);
        set(p, "j", 3.0);
        let o = create(p);
        set(o, "a", 0.0);
        assert_eq!(probe(o, "k"), 2.0f64.to_bits());
        crate::object::js_object_delete_field(
            p as *mut ObjectHeader,
            (atom("k") & POINTER_MASK) as *const crate::StringHeader,
        );
        assert_eq!(probe(o, "k"), 1.0f64.to_bits(), "the walk reaches g's k");
        assert_eq!(probe(o, "j"), 3.0f64.to_bits());
    });
}

/// A receiver with an overflow holding the key answers the overflow value.
#[test]
fn own_miss_with_an_overflow_reads_the_overflow() {
    on_a_fresh_thread(|| unsafe {
        let _ = realm();
        let p = crate::object::js_object_alloc(0, 1) as usize;
        set(p, "k", 5.0);
        let o = create(p);
        set(o, "a", 1.0);
        crate::object::overflow::insert_key_bits(
            o as *mut ObjectHeader,
            atom("k"),
            9.0f64.to_bits(),
            0,
        );
        assert_ne!(
            (*(o as *const ObjectHeader)).parent_class_id & crate::object::overflow::SHADOW_BIT,
            0
        );
        assert_eq!(agrees(o, "k"), 9.0f64.to_bits());
        assert_eq!(agrees(o, "zz"), TAG_UNDEFINED);
    });
}

/// An own list with a TEXT key: an atom read of its text answers the value
/// (or declines), and a list whose probe word lacks `PROBE_ATOMS` (a word no
/// compare can match) proves nothing by a miss.
#[test]
fn own_miss_on_a_text_key_list_declines() {
    on_a_fresh_thread(|| unsafe {
        let _ = realm();
        let o = crate::object::js_object_alloc(0, 2) as usize;
        set(o, "p2r_text_a", 1.0);
        let text = crate::string::js_string_from_bytes(b"p2r_xy".as_ptr(), 6);
        crate::object::js_object_set_field_by_name(o as *mut ObjectHeader, text, 4.0);
        let p = probe(o, "p2r_xy");
        assert!(p == TAG_HOLE || p == 4.0f64.to_bits(), "p2r_xy read {p:#x}");
        assert_eq!(get(o, "p2r_xy"), 4.0f64.to_bits());
        assert_eq!(agrees(o, "zz"), TAG_UNDEFINED);
        // With `PROBE_ATOMS` clear on the record (restored after; the layout
        // is this test's own) the own miss declines.
        let r = record_of(
            test_dir(),
            LayoutWord((*(o as *const ObjectHeader)).parent_class_id),
        );
        let pw = probe_word(r);
        assert!(pw & PROBE_READY != 0);
        (*r).probe = pw & !PROBE_ATOMS;
        let miss = probe(o, "zz");
        (*r).probe = pw;
        assert_eq!(
            miss, TAG_HOLE,
            "a list without PROBE_ATOMS proves nothing by a miss"
        );
    });
}

/// A non-ordinary receiver (an arguments object) keeps today's answer.
#[test]
fn own_miss_on_an_arguments_object_keeps_its_answer() {
    on_a_fresh_thread(|| unsafe {
        let _ = realm();
        let raw = crate::array::js_array_alloc(0) as usize;
        let args = crate::object::js_arguments_object_alloc(
            f64::from_bits(POINTER_TAG | raw as u64),
            f64::from_bits(TAG_UNDEFINED),
            1,
        ) as usize;
        for k in ["zz", "length", "toString", "callee"] {
            agrees(args, k);
        }
        assert_eq!(get(args, "zz"), TAG_UNDEFINED);
    });
}

/// A Proxy prototype is a boundary: the miss past the receiver declines.
#[test]
fn own_miss_before_a_proxy_prototype_declines() {
    on_a_fresh_thread(|| unsafe {
        let _ = realm();
        let target = crate::object::js_object_alloc(0, 0) as usize;
        let handler = crate::object::js_object_alloc(0, 0) as usize;
        let px = crate::proxy::js_proxy_new(
            f64::from_bits(POINTER_TAG | target as u64),
            f64::from_bits(POINTER_TAG | handler as u64),
        );
        let p = create(0);
        set(p, "q", 1.0);
        crate::object::js_object_set_prototype_of(f64::from_bits(POINTER_TAG | p as u64), px);
        let o = create(p);
        set(o, "a", 1.0);
        assert_eq!(probe(o, "q"), 1.0f64.to_bits());
        assert_eq!(probe(o, "zz"), TAG_HOLE, "the Proxy boundary declines");
    });
}

/// A getter on the prototype: the accessor arm answers (or declines).
#[test]
fn own_miss_reaching_a_prototype_getter_takes_the_accessor_arm() {
    on_a_fresh_thread(|| unsafe {
        let _ = realm();
        let p = crate::object::js_object_alloc(0, 1) as usize;
        set(p, "d", 1.0);
        let getter = global("Object");
        crate::object::js_object_define_getter(
            f64::from_bits(POINTER_TAG | p as u64),
            f64::from_bits(atom("g")),
            f64::from_bits(POINTER_TAG | getter as u64),
        );
        let o = create(p);
        set(o, "a", 1.0);
        let v = probe(o, "g");
        assert_ne!(v, TAG_UNDEFINED, "a getter is no absence");
        assert!(
            v == TAG_HOLE || v & !POINTER_MASK == POINTER_TAG,
            "g read {v:#x}"
        );
        assert_eq!(probe(o, "d"), 1.0f64.to_bits());
    });
}
