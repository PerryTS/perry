//! Object model §3.2: a layout key whose slot holds the hole is ABSENT, for
//! every reader of presence, order and value (`object::key_view`).
use super::*;
use crate::object::*;

thread_local! {
    static CTX: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    static FAILURES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

// Record a failed check and continue, so one run names every reader that
// fails; `finish` panics with the whole list.
fn fail(line: u32, what: String, note: &str) {
    let msg = format!(
        "[{}] line {line}: {what} {note}",
        CTX.with(|c| c.borrow().clone())
    );
    FAILURES.with(|f| f.borrow_mut().push(msg));
}
macro_rules! soft_eq {
    ($a:expr, $b:expr) => { soft_eq!($a, $b, "") };
    ($a:expr, $b:expr, $($m:tt)+) => {{
        let (a, b) = (&$a, &$b);
        if a != b {
            fail(line!(), format!("{:?} != {:?}", a, b), &format!($($m)+));
        }
    }};
}
macro_rules! soft_ne {
    ($a:expr, $b:expr) => { soft_ne!($a, $b, "") };
    ($a:expr, $b:expr, $($m:tt)+) => {{
        let (a, b) = (&$a, &$b);
        if a == b {
            fail(line!(), format!("{:?} == {:?}", a, b), &format!($($m)+));
        }
    }};
}
macro_rules! soft {
    ($c:expr) => { soft!($c, "") };
    ($c:expr, $($m:tt)+) => {{
        if !$c {
            fail(line!(), stringify!($c).to_string(), &format!($($m)+));
        }
    }};
}

fn ctx(label: &str) {
    CTX.with(|c| *c.borrow_mut() = label.to_string());
}

fn finish() {
    let failures = FAILURES.with(|f| std::mem::take(&mut *f.borrow_mut()));
    assert!(
        failures.is_empty(),
        "{} failed checks:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
runtime_atom!(H_A, "holes_a", "686f6c65735f61");
runtime_atom!(H_B, "holes_b", "686f6c65735f62");
runtime_atom!(H_C, "holes_c", "686f6c65735f63");
runtime_atom!(H_D, "holes_d", "686f6c65735f64");

const OPT: u8 = crate::object::key_attrs::ENTRY_OPTIONAL;

// Optional first and third keys, slot = position.
runtime_layout!(
    HOLEY,
    0x7f000011,
    0,
    crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    [H_A => OPT, H_B, H_C => OPT, H_D]
);

// The same keys with permuted slots: a's slot is 3, c's slot is 0.
runtime_layout!(
    HOLEY_PERMUTED,
    0x7f000012,
    0,
    crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    [H_A @ 3 => OPT, H_B @ 2, H_C @ 0 => OPT, H_D @ 1]
);

// No optional key: must keep every fast lane.
runtime_layout!(
    SOLID,
    0x7f000013,
    0,
    crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    [H_A, H_B, H_C, H_D]
);

// Same key schema, carried by a compiler-owned class family.
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
static mut CLASS_HOLEY: PerryLayoutDesc = PerryLayoutDesc {
    image_keys: std::ptr::null_mut(),
    shape_id: 0,
    gc_plan: 0,
    family: 0x41574203,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: HOLEY::SLOT_COUNT as u32,
    layout_id: 0x80000089,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: HOLEY::KEYS.as_ptr(),
    key_count: HOLEY::SLOT_COUNT as u32,
    index_mask: (HOLEY::INDEX.len() - 1) as u32,
    index: HOLEY::INDEX.as_ptr(),
    class_id: 60895,
    parent_class_id: 0,
    methods: std::ptr::null(),
    gc_header_word: crate::object::layout_desc::layout_gc_header_word(HOLEY::SLOT_COUNT as u32),
    _tail_pad: [0; 32],
};

// A constructor's `Object.defineProperty(this, "holes_b", { value })`: the
// key optional, carried with the definition's attributes (THR3).
const DEFINED: u8 = OPT
    | crate::object::key_attrs::ENTRY_NON_WRITABLE
    | crate::object::key_attrs::ENTRY_NON_ENUMERABLE
    | crate::object::key_attrs::ENTRY_NON_CONFIGURABLE;
runtime_layout!(
    DEFINED_HOLE,
    0x7f000018,
    0,
    crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    [H_A, H_B => DEFINED]
);

runtime_layout!(
    LITERAL_OPTIONAL,
    0x7f000014,
    0,
    crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    [H_A => OPT, H_B => OPT, H_C => OPT, H_D => OPT]
);

// Identical key geometry in an ordinary runtime layout is not family authority.
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_litlay")]
#[cfg_attr(target_os = "windows", link_section = ".perrylit$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_literal_layouts"
)]
static mut LITERAL_FAMILY: PerryLayoutDesc = PerryLayoutDesc {
    image_keys: std::ptr::null_mut(),
    shape_id: 0,
    gc_plan: 0,
    family: 0x7f000016,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: LITERAL_OPTIONAL::SLOT_COUNT as u32,
    layout_id: 0x8000008c,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: LITERAL_OPTIONAL::KEYS.as_ptr(),
    key_count: LITERAL_OPTIONAL::SLOT_COUNT as u32,
    index_mask: (LITERAL_OPTIONAL::INDEX.len() - 1) as u32,
    index: LITERAL_OPTIONAL::INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
    gc_header_word: crate::object::layout_desc::layout_gc_header_word(
        LITERAL_OPTIONAL::SLOT_COUNT as u32,
    ),
    _tail_pad: [0; 32],
};

const TRUE: u64 = crate::value::TAG_TRUE;

unsafe fn string(value: crate::JSValue) -> String {
    let mut sso = [0; crate::value::SHORT_STRING_MAX_LEN];
    String::from_utf8(
        crate::string::js_string_key_bytes(value, &mut sso)
            .unwrap_or_else(|| panic!("expected string, got {:016x}", value.bits()))
            .to_vec(),
    )
    .unwrap()
}

unsafe fn names(array: *mut crate::ArrayHeader) -> Vec<String> {
    (0..crate::array::js_array_length(array))
        .map(|i| string(crate::array::js_array_get(array, i)))
        .collect()
}

unsafe fn array_of(value: f64) -> *mut crate::ArrayHeader {
    (value.to_bits() & crate::value::POINTER_MASK) as *mut crate::ArrayHeader
}

unsafe fn json(value: f64) -> String {
    string(crate::JSValue::string_ptr(crate::json::js_json_stringify(
        value, 0,
    )))
}

/// Every enumeration reader lists exactly `expected`, in order.
unsafe fn assert_order(object: *mut ObjectHeader, expected: &[&str]) {
    assert_order_in(object, expected, expected)
}

/// [`assert_order`], with for-in's list (own keys, then inherited ones).
unsafe fn assert_order_in(object: *mut ObjectHeader, expected: &[&str], for_in: &[&str]) {
    let value = crate::js_nanbox_pointer(object as i64);
    soft_eq!(names(js_object_keys(object)), expected, "Object.keys");
    soft_eq!(
        names(js_object_keys_value(value)),
        expected,
        "Object.keys (value)"
    );
    soft_eq!(
        names(array_of(js_object_get_own_property_names(value))),
        expected,
        "getOwnPropertyNames"
    );
    soft_eq!(names(js_for_in_keys_value(value)), for_in, "for-in");
    soft_eq!(
        names(array_of(crate::proxy::js_reflect_own_keys(value))),
        expected,
        "Reflect.ownKeys"
    );
    soft_eq!(
        crate::array::js_array_length(js_object_values(object)) as usize,
        expected.len(),
        "Object.values"
    );
    let entries = js_object_entries(object);
    soft_eq!(
        crate::array::js_array_length(entries) as usize,
        expected.len(),
        "Object.entries"
    );
    for (i, name) in expected.iter().enumerate() {
        let pair = array_of(f64::from_bits(
            crate::array::js_array_get(entries, i as u32).bits(),
        ));
        soft_eq!(string(crate::array::js_array_get(pair, 0)), *name);
    }
    let descriptors = js_object_get_own_property_descriptors(value);
    let descriptor_keys = js_object_keys((descriptors.to_bits() & crate::value::POINTER_MASK) as _);
    soft_eq!(
        names(descriptor_keys),
        expected,
        "getOwnPropertyDescriptors"
    );
}

/// `key` is absent: no presence reader sees it.
unsafe fn assert_absent(object: *mut ObjectHeader, key: &StaticAtom) {
    let value = crate::js_nanbox_pointer(object as i64);
    let k = crate::js_nanbox_string(key.string as i64);
    let name = string(crate::JSValue::string_ptr(key.string.cast_mut()));
    soft_ne!(js_object_has_own(value, k).to_bits(), TRUE, "hasOwn {name}");
    soft_ne!(js_in_operator(value, k).to_bits(), TRUE, "in {name}");
    soft_ne!(
        js_object_has_property(value, k).to_bits(),
        TRUE,
        "has_property {name}"
    );
    soft_eq!(
        js_object_get_own_property_descriptor(value, k).to_bits(),
        crate::value::TAG_UNDEFINED,
        "getOwnPropertyDescriptor {name}"
    );
    soft_ne!(
        js_object_property_is_enumerable(value, k).to_bits(),
        TRUE,
        "propertyIsEnumerable {name}"
    );
}

unsafe fn assert_present(object: *mut ObjectHeader, key: &StaticAtom) {
    let value = crate::js_nanbox_pointer(object as i64);
    let k = crate::js_nanbox_string(key.string as i64);
    let name = string(crate::JSValue::string_ptr(key.string.cast_mut()));
    soft_eq!(js_object_has_own(value, k).to_bits(), TRUE, "hasOwn {name}");
    soft_eq!(js_in_operator(value, k).to_bits(), TRUE, "in {name}");
    soft_ne!(
        js_object_get_own_property_descriptor(value, k).to_bits(),
        crate::value::TAG_UNDEFINED,
        "getOwnPropertyDescriptor {name}"
    );
}

unsafe fn get(object: *mut ObjectHeader, key: &StaticAtom) -> u64 {
    js_object_get_field_by_name_f64(object, key.string).to_bits()
}

/// Born with every slot the hole; fill `b` and `d` only.
unsafe fn born(
    desc: *const PerryLayoutDesc,
    scope: &crate::gc::RuntimeHandleScope,
) -> crate::gc::RuntimeHandle<'_> {
    let object = scope.root_raw_mut_ptr(crate::object::layout_desc::perry_alloc_layout(desc));
    object.with_mut_ptr(|obj| {
        js_object_set_field_by_name(obj, H_B.string, 2.0);
        js_object_set_field_by_name(obj, H_D.string, 4.0);
    });
    object
}

#[test]
fn layout_holes_are_absent_for_every_reader() {
    crate::gc::initialize_runtime_fixture();
    for desc in [HOLEY::desc(), HOLEY_PERMUTED::desc()] {
        soft_ne!(
            unsafe { layout_id(desc) },
            0,
            "holey descriptor must register"
        );
        let scope = crate::gc::RuntimeHandleScope::new();
        let object = unsafe { born(desc, &scope) };
        unsafe {
            ctx(&format!("born {:#x}", (*desc).family));
            object.with_mut_ptr(|obj| {
                soft!(crate::object::key_view::object_may_have_holes(obj));
                assert_order(obj, &["holes_b", "holes_d"]);
                assert_absent(obj, &H_A);
                assert_absent(obj, &H_C);
                assert_present(obj, &H_B);
                assert_present(obj, &H_D);
                soft_eq!(get(obj, &H_A), crate::value::TAG_UNDEFINED);
                soft_eq!(get(obj, &H_B), 2.0f64.to_bits());
                let value = crate::js_nanbox_pointer(obj as i64);
                soft_eq!(json(value), "{\"holes_b\":2,\"holes_d\":4}");
                let inspected = crate::builtins::format_jsvalue(value, 0);
                soft!(!inspected.contains("holes_a"), "{inspected}");
                soft!(!inspected.contains("holes_c"), "{inspected}");
            });
            // Copies see only the present keys.
            ctx(&format!("structuredClone {:#x}", (*desc).family));
            let cloned = object.with_const_ptr(|obj: *const ObjectHeader| {
                crate::builtins::js_structured_clone(crate::js_nanbox_pointer(obj as i64))
            });
            let cloned = scope.root_nanbox_f64(cloned);
            assert_order(
                (cloned.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut ObjectHeader,
                &["holes_b", "holes_d"],
            );
            ctx(&format!("assign {:#x}", (*desc).family));
            let assigned = scope.root_raw_mut_ptr(js_object_alloc_plain(0));
            let result = object.with_const_ptr(|obj: *const ObjectHeader| {
                crate::object::js_object_assign_one(
                    crate::js_nanbox_pointer(assigned.get_raw_mut_ptr::<ObjectHeader>() as i64),
                    crate::js_nanbox_pointer(obj as i64),
                )
            });
            assert_order(
                (result.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader,
                &["holes_b", "holes_d"],
            );
            ctx(&format!("spread {:#x}", (*desc).family));
            let spread = object.with_const_ptr(|obj: *const ObjectHeader| {
                crate::object::spread::js_object_spread_fresh_leading(
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                    crate::js_nanbox_pointer(obj as i64),
                )
            });
            let spread = scope.root_nanbox_f64(spread);
            assert_order(
                (spread.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut ObjectHeader,
                &["holes_b", "holes_d"],
            );
            // A read of a hole continues to the prototype.
            ctx(&format!("proto {:#x}", (*desc).family));
            let proto = scope.root_raw_mut_ptr(js_object_alloc_plain(0));
            proto.with_mut_ptr(|p| js_object_set_field_by_name(p, H_A.string, 7.0));
            crate::object::js_object_set_prototype_of(
                crate::js_nanbox_pointer(object.get_raw_mut_ptr::<ObjectHeader>() as i64),
                crate::js_nanbox_pointer(proto.get_raw_mut_ptr::<ObjectHeader>() as i64),
            );
            object.with_mut_ptr(|obj| {
                soft_eq!(get(obj, &H_A), 7.0f64.to_bits(), "inherited through a hole");
                assert_absent_own(obj, &H_A);
                // for-in also lists the inherited key the hole lets through.
                assert_order_in(
                    obj,
                    &["holes_b", "holes_d"],
                    &["holes_b", "holes_d", "holes_a"],
                );
                for _ in 0..3 {
                    soft_eq!(
                        crate::object::field_get_set::runtime_read_site::get(obj, b"holes_a"),
                        7.0
                    );
                }
            });
            ctx(&format!("fill {:#x}", (*desc).family));
            // Filling a hole creates the key, which enumerates last, as
            // node's [[Set]] appends it.
            object.with_mut_ptr(|obj| {
                js_object_set_field_by_name(obj, H_C.string, 3.0);
                assert_order_in(
                    obj,
                    &["holes_b", "holes_d", "holes_c"],
                    &["holes_b", "holes_d", "holes_c", "holes_a"],
                );
                assert_present(obj, &H_C);
                soft_eq!(get(obj, &H_C), 3.0f64.to_bits());
            });
            ctx(&format!("delete {:#x}", (*desc).family));
            // A delete moves the object to its overflow; the remaining hole
            // stays absent.
            object.with_mut_ptr(|obj| {
                soft_eq!(js_object_delete_field(obj, H_B.string), 1);
                assert_order_in(
                    obj,
                    &["holes_d", "holes_c"],
                    &["holes_d", "holes_c", "holes_a"],
                );
                assert_absent_own(obj, &H_A);
                soft_eq!(get(obj, &H_A), 7.0f64.to_bits());
            });
            crate::gc::js_gc_collect();
            object.with_mut_ptr(|obj| {
                assert_order_in(
                    obj,
                    &["holes_d", "holes_c"],
                    &["holes_d", "holes_c", "holes_a"],
                )
            });
        }
    }
}

unsafe fn assert_absent_own(object: *mut ObjectHeader, key: &StaticAtom) {
    let value = crate::js_nanbox_pointer(object as i64);
    let k = crate::js_nanbox_string(key.string as i64);
    soft_ne!(js_object_has_own(value, k).to_bits(), TRUE);
    soft_eq!(
        js_object_get_own_property_descriptor(value, k).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    finish();
}

/// The state-free reads answer each object of a layout that can have holes
/// from its own slot: a read of an object whose optional key is present says
/// nothing about one of the same layout whose slot is a hole.
#[test]
fn layout_hole_reads_answer_per_object() {
    crate::gc::initialize_runtime_fixture();
    let desc = HOLEY::desc();
    let scope = crate::gc::RuntimeHandleScope::new();
    let full = unsafe { born(desc, &scope) };
    let holey = unsafe { born(desc, &scope) };
    unsafe {
        full.with_mut_ptr(|obj| js_object_set_field_by_name(obj, H_A.string, 1.0));
        let read = crate::object::field_get_set::runtime_read_site::get;
        for _ in 0..3 {
            full.with_mut_ptr(|obj| soft_eq!(read(obj, b"holes_a"), 1.0));
        }
        holey.with_mut_ptr(|obj| {
            soft_eq!(read(obj, b"holes_a").to_bits(), crate::value::TAG_UNDEFINED)
        });
        // The entry a site's declined probe calls.
        full.with_mut_ptr(|obj| {
            soft_eq!(
                crate::object::field_get_set::js_object_get_field_slow(
                    obj as *mut ObjectHeader as i64,
                    H_A.string
                ),
                1.0
            )
        });
        holey.with_mut_ptr(|obj| {
            soft_eq!(
                crate::object::field_get_set::js_object_get_field_slow(
                    obj as *mut ObjectHeader as i64,
                    H_A.string
                )
                .to_bits(),
                crate::value::TAG_UNDEFINED
            )
        });
        let k = crate::js_nanbox_string(H_A.string as i64);
        for _ in 0..3 {
            let v = full
                .with_const_ptr(|obj: *const ObjectHeader| crate::js_nanbox_pointer(obj as i64));
            soft_eq!(js_in_operator(v, k).to_bits(), TRUE);
        }
        let v =
            holey.with_const_ptr(|obj: *const ObjectHeader| crate::js_nanbox_pointer(obj as i64));
        soft_ne!(js_in_operator(v, k).to_bits(), TRUE);
    }
    finish();
}

/// A layout without optional keys has no hole bit: it keeps every lane.
#[test]
fn solid_layout_has_no_hole_bit() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope
        .root_raw_mut_ptr(unsafe { crate::object::layout_desc::perry_alloc_layout(SOLID::desc()) });
    unsafe {
        object.with_mut_ptr(|obj| {
            for (i, key) in [&H_A, &H_B, &H_C, &H_D].iter().enumerate() {
                js_object_set_field_by_name(obj, key.string, i as f64);
            }
            soft!(!crate::object::key_view::object_may_have_holes(obj));
            assert_order(obj, &["holes_a", "holes_b", "holes_c", "holes_d"]);
        });
    }
    finish();
}

/// A constructor may retire allocator placeholders before evaluating super.
/// Their absence must not create overflow or revoke the birth layout proof.
#[test]
fn deleting_absent_class_family_keys_preserves_birth_word() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = crate::object::layout_desc::perry_alloc_layout(std::ptr::addr_of!(CLASS_HOLEY));
        let word = (*obj).parent_class_id;
        for name in ["holes_a", "awb2_not_a_layout_key"] {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            assert_eq!(
                crate::object::delete_rest::js_object_delete_field(obj, key),
                1
            );
            assert_eq!((*obj).parent_class_id, word);
            assert!(!crate::object::overflow::has_overflow(obj));
        }
        let name = "holes_b";
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, 7.0);
        assert_eq!(
            crate::object::delete_rest::js_object_delete_field(obj, key),
            1
        );
        assert!(crate::object::overflow::has_overflow(obj));
        assert_ne!((*obj).parent_class_id, word);
    }
}

/// R1: a loop region over a class family is primed (the family's layout
/// keeps `SUMMARY_HOLES` for life), and its word carries the presence bit:
/// the shape cannot say which keys the object has, its slots do. The guard
/// proves presence once; inside F no JS runs, and a present key becomes absent
/// only through a delete, which must change the object's word.
#[test]
fn a_class_family_primes_a_loop_region_word_with_the_presence_test() {
    use crate::object::shapes::{
        js_region_loop_prime, object_shape_stamp, REGION_GUARD_WORD_EMPTY,
        REGION_LOOP_WORD_PRESENCE_TEST, REGION_LOOP_WORD_VALUE_TEST,
    };
    let _global = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let obj = crate::object::layout_desc::perry_alloc_layout(std::ptr::addr_of!(CLASS_HOLEY));
        js_object_set_field_by_name(obj, H_B.string, 7.0);
        let bits = |a: &StaticAtom| crate::js_nanbox_string(a.string as i64).to_bits();
        let site = core::sync::atomic::AtomicU64::new(REGION_GUARD_WORD_EMPTY);
        // The body reads `b` as a Number and stores `a`, still a hole.
        let word = js_region_loop_prime(
            &site,
            object_shape_stamp(obj).0,
            2,
            bits(&H_B),
            bits(&H_A),
            0,
            0,
            0,
            0,
            0b10,
            0,
            0b01,
        );
        assert_ne!(word, REGION_GUARD_WORD_EMPTY, "a class family is served");
        assert_eq!(word as u32, object_shape_stamp(obj).0);
        assert_ne!(word & REGION_LOOP_WORD_PRESENCE_TEST, 0);
        assert_ne!(word & REGION_LOOP_WORD_VALUE_TEST, 0);
        // The object decides presence: `b`'s slot holds its value, `a`'s the
        // hole, which the guard's presence test refuses.
        let slot = |i: u32| ((word >> (32 + 6 * i)) & 63) as usize;
        let fields = (obj as *const u8).add(core::mem::size_of::<ObjectHeader>()) as *const u64;
        assert_eq!(*fields.add(slot(0)), 7.0f64.to_bits());
        assert_eq!(*fields.add(slot(1)), crate::value::TAG_HOLE);
        let primed = object_shape_stamp(obj);
        assert_eq!(
            crate::object::delete_rest::js_object_delete_field(obj, H_B.string),
            1
        );
        assert_ne!(
            object_shape_stamp(obj),
            primed,
            "a delete of a present key must change the word the guard compares"
        );
    }
}

#[test]
fn ordinary_hole_delete_keeps_the_existing_order_representation() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = crate::object::layout_desc::perry_alloc_layout(HOLEY::desc());
        assert!(!crate::object::layout_desc::is_family_layout_word(
            (*obj).parent_class_id
        ));
        assert_eq!(
            crate::object::delete_rest::js_object_delete_field(obj, H_A.string),
            1
        );
        assert!(crate::object::overflow::has_overflow(obj));
    }
}

#[test]
fn absent_layout_methods_throw_or_resolve_an_inherited_method() {
    extern "C" fn return_receiver(
        _: *const crate::closure::ClosureHeader,
        this: crate::closure::JsThis,
    ) -> f64 {
        this.as_f64()
    }
    crate::gc::initialize_runtime_fixture();
    for descriptor in [HOLEY::desc(), HOLEY_PERMUTED::desc()] {
        let scope = crate::gc::RuntimeHandleScope::new();
        let object = unsafe { born(descriptor, &scope) };
        let receiver =
            || object.with_mut_ptr(|ptr: *mut ObjectHeader| crate::js_nanbox_pointer(ptr as i64));
        let missing = crate::exception::catch_js_throw(|| unsafe {
            crate::object::js_native_call_method(
                receiver(),
                b"holes_a".as_ptr().cast(),
                7,
                std::ptr::null(),
                0,
            )
        });
        assert!(
            missing.is_err(),
            "an absent method must throw rather than call the hole"
        );
        // A present method deleted into overflow must stop shadowing the
        // prototype, even though the immutable birth shape still names it.
        let own_method = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(return_receiver, 0),
            0,
        ) as *mut crate::closure::ClosureHeader);
        object.with_mut_ptr(|obj| {
            js_object_set_field_by_name(
                obj,
                H_A.string,
                own_method.with_mut_ptr(|ptr: *mut crate::closure::ClosureHeader| {
                    crate::js_nanbox_pointer(ptr as i64)
                }),
            );
        });
        // Setting a field can allocate; reload the receiver for the delete.
        object.with_mut_ptr(|obj| {
            assert_eq!(
                crate::object::delete_rest::js_object_delete_field(obj, H_A.string),
                1
            );
        });
        let deleted = crate::exception::catch_js_throw(|| unsafe {
            crate::object::js_native_call_method(
                receiver(),
                b"holes_a".as_ptr().cast(),
                7,
                std::ptr::null(),
                0,
            )
        });
        assert!(
            deleted.is_err(),
            "a deleted method must throw rather than call an obsolete shape slot"
        );
        let prototype = scope.root_raw_mut_ptr(js_object_alloc_plain(0));
        let method = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(return_receiver, 0),
            0,
        ) as *mut crate::closure::ClosureHeader);
        prototype.with_mut_ptr(|ptr| {
            js_object_set_field_by_name(
                ptr,
                H_A.string,
                method.with_mut_ptr(|ptr: *mut crate::closure::ClosureHeader| {
                    crate::js_nanbox_pointer(ptr as i64)
                }),
            );
        });
        crate::object::js_object_set_prototype_of(
            receiver(),
            prototype.with_mut_ptr(|ptr: *mut ObjectHeader| crate::js_nanbox_pointer(ptr as i64)),
        );
        let inherited = unsafe {
            crate::object::js_native_call_method(
                receiver(),
                b"holes_a".as_ptr().cast(),
                7,
                std::ptr::null(),
                0,
            )
        };
        assert_eq!(
            inherited.to_bits(),
            receiver().to_bits(),
            "inherited method receives the original this"
        );
    }
}

#[test]
fn classless_optional_family_keeps_shape_and_routes_unknown_keys_to_overflow() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let ordinary = scope.root_raw_mut_ptr(unsafe {
        crate::object::layout_desc::perry_alloc_layout(LITERAL_OPTIONAL::desc())
    });
    ordinary.with_const_ptr(|obj: *const ObjectHeader| unsafe {
        assert!(crate::object::layout_desc::family_desc_of(obj).is_none());
        assert_eq!(
            crate::object::layout_desc::family_own_key(obj, b"holes_a"),
            None,
            "an ordinary classless descriptor is not a family proof"
        );
        assert!(!crate::object::layout_desc::is_family_layout_word(
            (*obj).parent_class_id
        ));
    });
    let desc = std::ptr::addr_of!(LITERAL_FAMILY);
    let object = scope
        .root_raw_mut_ptr(unsafe { crate::object::layout_desc::perry_alloc_literal_layout(desc) });
    object.with_mut_ptr(|obj: *mut ObjectHeader| unsafe {
        assert_eq!((*obj).class_id, 0);
        let word = (*obj).parent_class_id;
        assert!(
            crate::object::layout_desc::is_family_layout_word(word),
            "must enter the classless family operation"
        );
        assert!(std::ptr::eq(
            crate::object::layout_desc::family_desc_of(obj).unwrap(),
            desc
        ));
        assert_absent_own(obj, &H_A);
        assert_eq!(
            crate::object::layout_desc::family_own_key(obj, b"holes_a"),
            Some(Some((0, 0)))
        );
        assert_eq!(
            crate::object::layout_desc::family_own_key(obj, b"not_in_family"),
            Some(None),
            "a family proof can answer absence without an ordinary probe"
        );
        js_object_set_field_by_name(obj, H_A.string, 17.0);
        js_object_set_field_by_name(obj, H_B.string, 23.0);
        assert_eq!(
            (*obj).parent_class_id,
            word,
            "ordered fills keep the family shape"
        );
        assert_order(obj, &["holes_a", "holes_b"]);
    });
    let extra = scope.root_string_ptr(crate::string::js_string_from_bytes(
        b"holes_extra".as_ptr(),
        11,
    ));
    object.with_mut_ptr(|obj: *mut ObjectHeader| {
        extra.with_const_ptr(|name| js_object_set_field_by_name(obj, name, 31.0));
    });
    object.with_mut_ptr(|obj: *mut ObjectHeader| unsafe {
        assert!(
            !(*obj).meta.is_null() && (*(*obj).meta).overflow != 0,
            "an unmodeled key must enter the authoritative overflow"
        );
        assert_order(obj, &["holes_a", "holes_b", "holes_extra"]);
        assert_eq!(js_object_get_field_by_name_f64(obj, H_A.string), 17.0);
    });
    crate::gc::js_gc_collect();
    object.with_mut_ptr(|obj: *mut ObjectHeader| unsafe {
        assert_order(obj, &["holes_a", "holes_b", "holes_extra"]);
    });
    object.with_mut_ptr(|obj: *mut ObjectHeader| {
        extra.with_const_ptr(|name| {
            assert_eq!(js_object_get_field_by_name_f64(obj, name), 31.0);
        });
    });
    let extra_atom = extra.with_const_ptr(|name| unsafe { crate::string::property_atom(name) });
    object.with_mut_ptr(|obj: *mut ObjectHeader| unsafe {
        let atom = crate::string::property_atom(H_A.string);
        let key = crate::JSValue::string_ptr(atom.cast_mut()).bits();
        let hash = crate::string::property_atom_hash(atom);
        let receiver = crate::value::POINTER_TAG | obj as u64;
        let dir = crate::object::shapes::ordinary_dir_addr();
        assert_eq!(
            crate::object::layout_probe::perry_probe_set(dir, receiver, key, hash, 41.0),
            1,
            "a writable overflow reference must use the leaf slot store"
        );
        assert_eq!(js_object_get_field_by_name_f64(obj, H_A.string), 41.0);
        assert_eq!(
            crate::object::layout_probe::perry_probe_set(dir, receiver, key, hash, 17.0),
            1
        );
        let atom = extra_atom;
        let key = crate::JSValue::string_ptr(atom.cast_mut()).bits();
        let hash = crate::string::property_atom_hash(atom);
        assert_eq!(
            crate::object::layout_probe::perry_probe_set(dir, receiver, key, hash, 43.0),
            1,
            "a table-owned value keeps its plain-value leaf store"
        );
        extra.with_const_ptr(|name| {
            assert_eq!(js_object_get_field_by_name_f64(obj, name), 43.0);
        });
        assert_eq!(
            crate::object::layout_probe::perry_probe_set(dir, receiver, key, hash, 31.0),
            1
        );
        crate::object::set_property_attrs(
            obj as usize,
            "holes_a".into(),
            crate::object::PropertyAttrs::new(false, true, true),
        );
        let shape = crate::object::shapes::object_shape_descriptor(obj).unwrap();
        assert_eq!(
            shape.object_kind,
            crate::object::shapes::ShapeObjectKind::Ordinary
        );
        assert!(crate::object::overflow::has_overflow(obj));
        assert_eq!(
            crate::object::key_attrs::keys_entry(shape.keys as *const crate::array::ArrayHeader, 0)
                & crate::object::key_attrs::ENTRY_NON_WRITABLE,
            0,
            "the immutable layout alone would incorrectly admit this store"
        );
        assert!(!crate::object::get_property_attrs(obj as usize, "holes_a")
            .unwrap()
            .writable());
        let atom = crate::string::property_atom(H_A.string);
        assert_eq!(
            crate::object::layout_probe::perry_probe_set(
                crate::object::shapes::ordinary_dir_addr(),
                crate::value::POINTER_TAG | obj as u64,
                crate::JSValue::string_ptr(atom.cast_mut()).bits(),
                crate::string::property_atom_hash(atom),
                99.0,
            ),
            0,
            "overflow attributes forbid the reserved-slot store"
        );
        assert_eq!(js_object_get_field_by_name_f64(obj, H_A.string), 17.0);
    });
    let mixed = scope
        .root_raw_mut_ptr(unsafe { crate::object::layout_desc::perry_alloc_layout(HOLEY::desc()) });
    mixed.with_const_ptr(|obj: *const ObjectHeader| unsafe {
        assert!(
            crate::object::layout_desc::family_desc_of(obj).is_none(),
            "mixed required/optional runtime layouts are not newly admitted"
        );
    });
    finish();
}

// No eager counterpart shares these keys: the second agent must materialize
// its own record while the immutable image word is shared by both agents.
#[cfg(not(target_family = "wasm"))]
mod first_birth {
    use super::*;

    runtime_atom!(LAZY_A, "first_birth_a", "66697273745f62697274685f61");
    runtime_atom!(LAZY_B, "first_birth_b", "66697273745f62697274685f62");

    const ATOMS: [LayoutAtom; 2] = [
        (std::ptr::addr_of!(LAZY_A), LAZY_A::HASH, OPT, None),
        (std::ptr::addr_of!(LAZY_B), LAZY_B::HASH, OPT, None),
    ];
    static KEYS: [PerryLayoutKey; 2] = layout_keys(ATOMS);
    static INDEX: [u32; 4] = layout_index(&ATOMS);

    #[used(linker)]
    #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_litlay")]
    #[cfg_attr(target_os = "windows", link_section = ".perrylit$m")]
    #[cfg_attr(
        not(any(target_vendor = "apple", target_os = "windows")),
        link_section = "perry_literal_layouts"
    )]
    static mut LITERAL: PerryLayoutDesc = PerryLayoutDesc {
        image_keys: std::ptr::null_mut(),
        shape_id: 0,
        gc_plan: 0,
        family: 0x7f000015,
        variant: 0,
        kind: 0,
        integrity: 0,
        slot_count: 2,
        layout_id: 0x8000008d,
        rep: 0,
        proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
        keys: KEYS.as_ptr(),
        key_count: 2,
        index_mask: 3,
        index: INDEX.as_ptr(),
        class_id: 0,
        parent_class_id: 0,
        methods: std::ptr::null(),
        gc_header_word: crate::object::layout_desc::layout_gc_header_word(2),
        _tail_pad: [0; 32],
    };

    #[test]
    fn unused_literal_stays_static_and_first_birth_installs_each_agents_record() {
        crate::gc::initialize_runtime_fixture();
        let desc = std::ptr::addr_of!(LITERAL);
        unsafe {
            assert_eq!(
                std::sync::atomic::AtomicU32::from_ptr(
                    std::ptr::addr_of!((*desc).shape_id).cast_mut(),
                )
                .load(std::sync::atomic::Ordering::Acquire),
                0,
                "ordinary startup must not publish an unused literal identity"
            );
            let scope = crate::gc::RuntimeHandleScope::new();
            let obj = scope
                .root_raw_mut_ptr(crate::object::layout_desc::perry_alloc_literal_layout(desc));
            let id = obj.with_const_ptr(|p: *const ObjectHeader| (*p).parent_class_id);
            assert!(
                crate::object::shapes::shape_descriptor_for_word(perry_abi::LayoutWord(id))
                    .is_some()
            );
            assert_eq!(crate::object::layout_desc::load_layout_id(desc), id);
            assert_ne!(
                id,
                (*desc).shape_id,
                "DPA row and ShapeId identity stay separate"
            );
            obj.with_mut_ptr(|p: *mut ObjectHeader| {
                assert!(std::ptr::eq(
                    crate::object::layout_desc::family_desc_of(p).unwrap(),
                    desc
                ));
                js_object_set_field_by_name(p, LAZY_A.string, 17.0);
                assert_eq!(js_object_get_field_by_name_f64(p, LAZY_A.string), 17.0);
            });
            std::thread::spawn(move || {
                crate::gc::initialize_runtime_fixture();
                let desc = std::ptr::addr_of!(LITERAL);
                assert_eq!(crate::object::layout_desc::load_layout_id(desc), id);
                assert!(
                    crate::object::shapes::shape_descriptor_for_word(perry_abi::LayoutWord(id))
                        .is_none(),
                    "a shared image word must not stand in for this agent's record"
                );
                let scope = crate::gc::RuntimeHandleScope::new();
                let obj = scope
                    .root_raw_mut_ptr(crate::object::layout_desc::perry_alloc_literal_layout(desc));
                assert!(
                    crate::object::shapes::shape_descriptor_for_word(perry_abi::LayoutWord(id))
                        .is_some()
                );
                obj.with_mut_ptr(|p: *mut ObjectHeader| {
                    assert_eq!((*p).parent_class_id, id);
                    assert!(std::ptr::eq(
                        crate::object::layout_desc::family_desc_of(p).unwrap(),
                        desc
                    ));
                    js_object_set_field_by_name(p, LAZY_B.string, 23.0);
                    assert_eq!(js_object_get_field_by_name_f64(p, LAZY_B.string), 23.0);
                });
            })
            .join()
            .unwrap();
        }
    }
}

#[test]
fn absent_overflow_definition_uses_final_data_entry() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    unsafe {
        let object =
            scope.root_raw_mut_ptr(crate::object::layout_desc::perry_alloc_literal_layout(
                std::ptr::addr_of!(LITERAL_FAMILY),
            ));
        object.with_mut_ptr(|obj| {
            crate::object::overflow::ensure(obj);
        });
        let receiver = scope.root_nanbox_u64(
            object
                .with_const_ptr(|obj: *const ObjectHeader| crate::value::POINTER_TAG | obj as u64),
        );
        let key = scope.root_nanbox_u64(crate::JSValue::string_ptr(H_C.string.cast_mut()).bits());
        let bag = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
        for (name, value) in [
            (b"value".as_slice(), 37.0),
            (b"enumerable", f64::from_bits(TRUE)),
        ] {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            bag.with_mut_ptr(|obj| js_object_set_field_by_name(obj, key, value));
        }
        let bag_value = scope.root_nanbox_u64(
            bag.with_const_ptr(|obj: *const ObjectHeader| crate::value::POINTER_TAG | obj as u64),
        );
        let desc = crate::object::object_ops::decode_property_descriptor(&scope, &bag_value);
        assert!(
            crate::object::define_own_data::define_fresh_own_from_shape(
                &scope, &receiver, &key, &desc,
            ),
            "an absent reserved key must use the fresh overflow definition"
        );
        object.with_const_ptr(|obj: *const ObjectHeader| {
            let table = crate::object::overflow::table(obj);
            let i = crate::object::overflow::find(table, key.get_nanbox_u64()).unwrap();
            assert!(crate::object::overflow::fixed_slot_of(table, i).is_none());
            assert_eq!(
                crate::object::overflow::get(obj, i),
                Some(37.0f64.to_bits())
            );
            assert_eq!(
                crate::object::overflow::entry_attrs(table.cast(), i),
                crate::object::key_attrs::ENTRY_NON_WRITABLE
                    | crate::object::key_attrs::ENTRY_NON_CONFIGURABLE
            );
            assert_eq!(
                crate::object::field_get_set::object_field_at_with_live(
                    obj,
                    2,
                    crate::object::object_live_slot_count(obj),
                )
                .bits(),
                crate::value::TAG_HOLE,
                "the reserved physical slot stays absent; the overflow owns its value",
            );
        });
        assert!(
            !crate::object::define_own_data::define_fresh_own_from_shape(
                &scope, &receiver, &key, &desc,
            ),
            "present overflow keys retain complete definition semantics"
        );
        let unreserved = scope.root_nanbox_u64(
            crate::JSValue::string_ptr(
                crate::string::intern_ascii_literal(b"first_unreserved").cast_mut(),
            )
            .bits(),
        );
        let first = scope.root_raw_mut_ptr(crate::object::layout_desc::perry_alloc_literal_layout(
            std::ptr::addr_of!(LITERAL_FAMILY),
        ));
        first.with_const_ptr(|obj: *const ObjectHeader| {
            assert!(!crate::object::overflow::has_overflow(obj));
        });
        let first_value = scope.root_nanbox_u64(
            first.with_const_ptr(|obj: *const ObjectHeader| crate::value::POINTER_TAG | obj as u64),
        );
        assert!(
            crate::object::define_own_data::define_fresh_own_from_shape(
                &scope,
                &first_value,
                &unreserved,
                &desc,
            ),
            "the first unreserved family definition must use the same fresh insertion"
        );
        assert!(
            !crate::object::define_own_data::define_fresh_own_from_shape(
                &scope,
                &first_value,
                &unreserved,
                &desc,
            ),
            "an interned non-atom spelling must find the existing authoritative property"
        );
        first.with_const_ptr(|obj: *const ObjectHeader| {
            let table = crate::object::overflow::table(obj);
            assert!(!table.is_null());
            let atom = crate::string::existing_property_atom(
                crate::JSValue::from_bits(unreserved.get_nanbox_u64()).as_string_ptr(),
            )
            .unwrap();
            let i = crate::object::overflow::find(
                table,
                crate::JSValue::string_ptr(atom.cast_mut()).bits(),
            )
            .unwrap();
            assert_eq!(
                crate::object::overflow::get(obj, i),
                Some(37.0f64.to_bits())
            );
            assert_eq!(
                crate::object::overflow::entry_attrs(table.cast(), i),
                crate::object::key_attrs::ENTRY_NON_WRITABLE
                    | crate::object::key_attrs::ENTRY_NON_CONFIGURABLE
            );
        });
    }
}

static LITERAL_FAMILY_DECLARATION: crate::object::class_registry::declarations::ClassDeclaration =
    crate::object::class_registry::declarations::ClassDeclaration {
        instance_layout: std::ptr::addr_of!(LITERAL_FAMILY),
        ..crate::object::class_registry::declarations::ClassDeclaration::native(&[])
    };

/// A literal family's objects carry class id 0: the proven class is told by the
/// family descriptor word its declaration owns (the runtime fact check's F4).
#[test]
fn a_literal_family_object_is_its_proven_class_by_the_layout_word() {
    const CLASS: u32 = 60_301;
    crate::gc::initialize_runtime_fixture();
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    unsafe {
        crate::object::class_registry::declarations::js_register_class_declaration(
            CLASS,
            &LITERAL_FAMILY_DECLARATION,
        )
    };
    let literal = scope.root_raw_mut_ptr(unsafe {
        crate::object::layout_desc::perry_alloc_literal_layout(std::ptr::addr_of!(LITERAL_FAMILY))
    });
    let ordinary = scope.root_raw_mut_ptr(unsafe {
        crate::object::layout_desc::perry_alloc_layout(std::ptr::addr_of!(LITERAL_OPTIONAL))
    });
    let (class_id, word) = literal.with_const_ptr(|obj: *const ObjectHeader| unsafe {
        ((*obj).class_id, (*obj).parent_class_id)
    });
    assert_eq!(class_id, 0, "a literal family names class 0");
    assert!(crate::object::layout_desc::word_is_class_family_layout(
        word, CLASS
    ));
    assert!(
        !crate::object::layout_desc::word_is_class_family_layout(word, CLASS + 1),
        "a class without that declaration"
    );
    let other =
        ordinary.with_const_ptr(|obj: *const ObjectHeader| unsafe { (*obj).parent_class_id });
    assert!(
        !crate::object::layout_desc::word_is_class_family_layout(other, CLASS),
        "an ordinary classless layout is not the class's family"
    );
}

runtime_atom!(H_LATE, "holes_late", "686f6c65735f6c617465");

// An array creation family's named layout as perry-codegen emits it: the
// iterator key, then a key a store site declares ahead of its first store
// (optional, its slot born holding the hole).
runtime_layout!(
    ARRAY_LATE,
    0x7f000017,
    0,
    crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    [ATOM_SYMBOL_ITERATOR, H_LATE => OPT]
);

/// An array layout's declared late key is absent until its first store, so
/// that store is an add: it asks the prototype chain (an inherited read-only
/// key decides it) and, when nothing intercepts, fills the listed slot.
#[test]
fn an_array_layout_late_key_is_absent_and_its_store_asks_the_chain() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _ = js_get_global_this();
    unsafe {
        let key = crate::value::js_nanbox_string(H_LATE.string as i64);
        let fresh = || {
            let arr = crate::value::js_nanbox_pointer(crate::array::js_array_alloc(0) as i64);
            crate::array::named_props::perry_array_attach_layout(arr, ARRAY_LATE::desc())
        };
        let bag_of = |arr: f64| {
            crate::array::array_property_bag(
                (arr.to_bits() & crate::value::POINTER_MASK) as *const crate::ArrayHeader,
            )
        };
        let bag_value = |arr: f64| crate::value::js_nanbox_pointer(bag_of(arr) as i64);
        let late_slot = |arr: f64| {
            *((bag_of(arr) as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64)
                .add(1)
        };

        let a = fresh();
        assert_eq!(late_slot(a), crate::value::TAG_HOLE);
        assert!(
            !crate::object::shape_store::try_own_store(bag_of(a), H_LATE.string, 1.0),
            "an absent optional key is no own overwrite"
        );
        assert!(
            !obj_value_has_own_key(bag_value(a), key),
            "a declared late key whose slot holds the hole is absent"
        );
        assert!(
            crate::object::property_ops::set::own_set_descriptor(a, key).is_none(),
            "[[Set]]'s own-descriptor read walks on to the prototype"
        );
        assert_eq!(crate::object::shape_store::try_store(a, key, 1.0), Some(1.0));
        assert_eq!(late_slot(a), 1.0_f64.to_bits(), "the add fills the listed slot");
        assert!(obj_value_has_own_key(bag_value(a), key));
        assert_eq!(object_keys(bag_of(a)).count(), 2, "the add appended no key");
        assert!(
            crate::object::shape_store::try_own_store(bag_of(a), H_LATE.string, 2.0),
            "a present optional key: its store is the plain overwrite"
        );
        assert_eq!(late_slot(a), 2.0_f64.to_bits());

        let proto = crate::array::object_prototype_addr() as *mut ObjectHeader;
        // Decision 250: Object.prototype is sealed, so adding and removing the
        // inherited read-only key are runtime definitions (the test models a
        // builtin member; the point is the add asking the chain).
        crate::object::own_override::as_builtin_definition(|| {
            js_object_set_field_by_name(proto, H_LATE.string, 5.0)
        });
        crate::object::own_override::as_builtin_definition(|| {
            set_property_attrs(
                proto as usize,
                "holes_late".into(),
                PropertyAttrs::new(false, true, true),
            )
        });
        let b = fresh();
        let result = crate::object::shape_store::try_store(b, key, 9.0);
        let slot_after = late_slot(b);
        let present = obj_value_has_own_key(bag_value(b), key);
        crate::object::own_override::as_builtin_definition(|| {
            js_object_delete_dynamic_value(crate::value::js_nanbox_pointer(proto as i64), key)
        });
        assert_eq!(result, None, "an inherited read-only key decides the add");
        assert_eq!(slot_after, crate::value::TAG_HOLE, "the declined add stored nothing");
        assert!(!present);
    }
}

/// The definition a layout carries an optional key's attributes for fills
/// its hole in place (THR3), and sets no `OBJ_FLAG_HAS_DESCRIPTORS`: the
/// shape carries the attributes, as for an object born with them, so a field
/// guard (`GC_OBJECT_FIELD_GUARD_MASK_I32`, which includes the bit) keeps its
/// fast path on the filled object. Every attribute reader reads the shape.
#[test]
fn a_defined_hole_fill_keeps_the_field_guard_bit_clear() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(unsafe {
        crate::object::layout_desc::perry_alloc_layout(DEFINED_HOLE::desc())
    });
    let descriptor_bit = |obj: *const ObjectHeader| unsafe {
        (*crate::gc::header_from_trusted_user_ptr(obj.cast()))._reserved
            & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
    };
    let word = object.with_const_ptr(|obj: *const ObjectHeader| unsafe {
        assert_eq!(descriptor_bit(obj), 0, "born without the bit");
        assert_absent_own(obj as *mut ObjectHeader, &H_B);
        (*obj).parent_class_id
    });
    let descriptor = scope.root_raw_mut_ptr(crate::object::js_object_alloc_null_proto(0, 0));
    descriptor.with_mut_ptr(|d: *mut ObjectHeader| {
        js_object_set_field_by_name(d, crate::string::intern_ascii_literal(b"value"), 5.0)
    });
    let receiver =
        object.with_const_ptr(|obj: *const ObjectHeader| crate::js_nanbox_pointer(obj as i64));
    let bag =
        descriptor.with_const_ptr(|d: *const ObjectHeader| crate::js_nanbox_pointer(d as i64));
    let key = crate::js_nanbox_string(H_B.string as i64);
    crate::object::js_object_define_property(receiver, key, bag);
    object.with_mut_ptr(|obj: *mut ObjectHeader| unsafe {
        assert_eq!(
            (*obj).parent_class_id,
            word,
            "the definition filled the slot in place"
        );
        assert_eq!(
            descriptor_bit(obj),
            0,
            "the shape carries the attributes; no header bit"
        );
        assert_eq!(js_object_get_field_by_name_f64(obj, H_B.string), 5.0);
        assert_eq!(
            crate::object::descriptor_state::get_property_attrs_bytes(obj as usize, b"holes_b")
                .map(|a| (a.writable(), a.enumerable(), a.configurable())),
            Some((false, false, false)),
            "the key has the definition's attributes"
        );
    });
}
