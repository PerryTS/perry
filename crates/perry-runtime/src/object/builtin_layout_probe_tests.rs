//! Step 6 slice 3b-i: `%Object.prototype%`, `%Function.prototype%` and
//! `%Array.prototype%`'s holder are static layouts, and the probe answers
//! each hop through them from the layout. A program's write to one of them
//! must be read back (or declined), never answered from the pristine layout.
use super::*;
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
            (op as *const ObjectHeader, std::ptr::addr_of!(crate::object::runtime_layouts::OBJECT_PROTOTYPE)),
            (fp as *const ObjectHeader, std::ptr::addr_of!(crate::object::runtime_layouts::FUNCTION_PROTOTYPE)),
            (holder as *const ObjectHeader, std::ptr::addr_of!(crate::object::runtime_layouts::ARRAY_PROTOTYPE_NAMED)),
        ] {
            let r = record_of(test_dir(), (*obj).parent_class_id);
            assert!(!r.is_null());
            let d = &*desc;
            let declared = record_of(test_dir(), d.layout_id);
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
        // (`Array` carries an accessor, `Array[Symbol.species]`: its own
        // properties are answered by the object, and the probe may decline.)
        assert_ne!(agrees(array_ctor, "isBuffer"), POINTER_TAG);
        // Inherited through %Function.prototype%.
        assert_ne!(agrees(object_ctor, "hasOwnProperty"), TAG_UNDEFINED);
        // An own bag key is read; a name a function answers itself outside
        // its bag is declined.
        agrees(object_ctor, "name");
        assert_eq!(probe(object_ctor, "call"), TAG_HOLE);
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
        set(fp, "isBuffer", 7.0);
        assert_eq!(get(object_ctor, "isBuffer"), 7.0f64.to_bits());
        agrees(object_ctor, "isBuffer");
        set(op, "extra", 8.0);
        for obj in [arr, o, object_ctor] {
            assert_eq!(get(obj, "extra"), 8.0f64.to_bits());
            agrees(obj, "extra");
        }
        crate::array::named_props::array_named_property_set(
            ap as *mut crate::array::ArrayHeader,
            (atom("map") & POINTER_MASK) as *const crate::StringHeader,
            9.0,
        );
        assert_eq!(get(arr, "map"), 9.0f64.to_bits());
        agrees(arr, "map");
        for p in [op, fp] {
            assert!(crate::object::layout_desc::word_is_shadowed(
                (*(p as *const ObjectHeader)).parent_class_id
            ));
        }
    });
}

#[test]
fn a_relink_of_array_prototype_stops_the_holder_answering() {
    on_a_fresh_thread(|| unsafe {
        let (op, _fp, ap) = realm();
        let arr = crate::array::js_array_alloc(0) as usize;
        set(op, "onObjectPrototype", 3.0);
        assert_eq!(agrees(arr, "onObjectPrototype"), 3.0f64.to_bits());
        // `Object.setPrototypeOf(Array.prototype, P)`, P with a null
        // [[Prototype]]: `%Object.prototype%` is no longer on any array's
        // chain.
        let p = crate::object::js_object_alloc(0, 1);
        crate::object::object_ops::js_object_set_prototype_of(
            f64::from_bits(POINTER_TAG | p as u64),
            f64::from_bits(crate::value::TAG_NULL),
        );
        set(p as usize, "viaP", 5.0);
        crate::object::object_ops::js_object_set_prototype_of(
            f64::from_bits(POINTER_TAG | ap as u64),
            f64::from_bits(POINTER_TAG | p as u64),
        );
        // The holder's shadow bit says every array's chain left its layout:
        // the probe declines, whatever the complete read answers.
        let holder = crate::array::array_property_bag(ap as *const crate::array::ArrayHeader);
        assert!(crate::object::layout_desc::word_is_shadowed((*holder).parent_class_id));
        assert_eq!(probe(arr, "onObjectPrototype"), TAG_HOLE);
        assert_eq!(probe(arr, "viaP"), TAG_HOLE);
    });
}

#[test]
fn a_deleted_and_readded_builtin_is_read_back() {
    on_a_fresh_thread(|| unsafe {
        let (op, fp, _ap) = realm();
        let object_ctor = global("Object");
        let arr = crate::array::js_array_alloc(0) as usize;
        let o = crate::object::js_object_alloc(0, 1) as usize;
        for (proto, key) in [(op, "toLocaleString"), (fp, "apply"), (op, "propertyIsEnumerable")] {
            crate::object::js_object_delete_field(
                proto as *mut ObjectHeader,
                (atom(key) & POINTER_MASK) as *const crate::StringHeader,
            );
            for obj in [o, arr, object_ctor] {
                agrees(obj, key);
            }
            set(proto, key, 11.0);
            eprintln!(
                "readded {key}: overflow={}",
                !crate::object::overflow::table(proto as *const ObjectHeader).is_null()
            );
            for obj in [o, arr, object_ctor] {
                agrees(obj, key);
            }
            assert_eq!(get(o, key), if proto == op { 11.0f64.to_bits() } else { get(o, key) });
        }
        // After a delete the overflow is the order authority: a new key
        // lands there, off the layout, and the shadow bit sends the probe
        // to it.
        for (proto, key) in [(op, "addedAfterDelete"), (fp, "addedAfterDelete2")] {
            set(proto, key, 12.0);
            for obj in [o, arr, object_ctor] {
                if proto == op || obj == object_ctor {
                    assert_eq!(get(obj, key), 12.0f64.to_bits());
                }
                agrees(obj, key);
            }
        }
    });
}
