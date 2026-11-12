//! #13486: `Array.from(primitive)` — GetMethod through the `ToObject` wrapper.
//!
//! The non-mapped `js_array_from_value` used to reach the pointer tail with a
//! primitive's raw bits (a number's bits are not an address), so `Array.from(5)`
//! segfaulted and a patched `Number.prototype[Symbol.iterator]` was never
//! consulted. The contract pinned here:
//!   - unpatched `Array.from(<number / boolean / symbol>)` yields `[]`,
//!   - `Array.from("abc")` yields code points, honoring a patched
//!     `String.prototype[Symbol.iterator]`,
//!   - a patched `Number.prototype[Symbol.iterator]` drives `Array.from(5)`,
//!   - the mapped form still applies `mapFn` with the right index.

use super::*;
use crate::closure::{js_closure_alloc, ClosureHeader};
use crate::value::JSValue;
use std::cell::{Cell, RefCell};

struct TestScope {
    _lock: std::sync::MutexGuard<'static, ()>,
    _no_gc: crate::gc::GcSuppressScope,
    _global: *mut crate::object::ObjectHeader,
}

impl TestScope {
    fn new() -> Self {
        Self {
            _lock: crate::gc::global_side_table_test_lock(),
            _no_gc: crate::gc::GcSuppressScope::new(),
            _global: {
                let global = crate::object::js_object_alloc(0, 0);
                crate::object::populate_global_this_builtins(global);
                global
            },
        }
    }
}

thread_local! {
    static NEXT_CALLS: Cell<u32> = const { Cell::new(0) };
    static MAP_CALLS: RefCell<Vec<(f64, f64)>> = const { RefCell::new(Vec::new()) };
}

fn iterator_symbol_value() -> f64 {
    let sym = crate::symbol::well_known_symbol("iterator");
    assert!(!sym.is_null(), "well-known @@iterator must exist");
    f64::from_bits(JSValue::pointer(sym as *const u8).bits())
}

fn number_prototype_value() -> f64 {
    let addr = crate::array::primitive_wrapper_prototype_addr(5.0);
    assert!(addr != 0, "%Number.prototype% must be resolvable");
    f64::from_bits(JSValue::pointer(addr as *const u8).bits())
}

fn boxed_two_value_iterator() -> f64 {
    let raw = js_closure_alloc(crate::fn_info!(two_value_iterator, 0), 0);
    f64::from_bits(JSValue::pointer(raw as *const u8).bits())
}

fn boxed_map_fn_times10() -> f64 {
    let raw = js_closure_alloc(crate::fn_info!(map_fn_times10_plus_index, 2), 0);
    f64::from_bits(JSValue::pointer(raw as *const u8).bits())
}

fn boxed_map_fn_record_index() -> f64 {
    let raw = js_closure_alloc(crate::fn_info!(map_fn_record_index, 2), 0);
    f64::from_bits(JSValue::pointer(raw as *const u8).bits())
}

unsafe fn patch_prototype_iterator(proto: f64, method: f64) {
    crate::object::own_override::as_builtin_definition(|| {
        crate::symbol::js_object_set_symbol_property(proto, iterator_symbol_value(), method)
    });
}

fn make_obj() -> *mut crate::object::ObjectHeader {
    crate::object::js_object_alloc(0, 4)
}

unsafe fn set_field(obj: *mut crate::object::ObjectHeader, name: &[u8], value: f64) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

/// `next` yielding 40, 41, then done — a patched prototype's iterator.
extern "C" fn patched_next(_c: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    let n = NEXT_CALLS.with(|c| {
        let v = c.get();
        c.set(v + 1);
        v
    });
    let obj = make_obj();
    unsafe {
        if n < 2 {
            set_field(obj, b"value", 40.0 + n as f64);
            set_field(obj, b"done", f64::from_bits(crate::value::TAG_FALSE));
        } else {
            set_field(obj, b"done", f64::from_bits(crate::value::TAG_TRUE));
        }
    }
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

extern "C" fn two_value_iterator(_c: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    NEXT_CALLS.with(|c| c.set(0));
    let obj = make_obj();
    let next = js_closure_alloc(crate::fn_info!(patched_next, 0), 0);
    unsafe {
        set_field(obj, b"next", f64::from_bits(JSValue::pointer(next as *const u8).bits()));
    }
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

extern "C" fn map_fn_times10_plus_index(
    _c: *const ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
    index: f64,
) -> f64 {
    MAP_CALLS.with(|v| v.borrow_mut().push((value, index)));
    value * 10.0 + index
}

extern "C" fn map_fn_record_index(
    _c: *const ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
    index: f64,
) -> f64 {
    MAP_CALLS.with(|v| v.borrow_mut().push((value, index)));
    value
}

fn take_map_calls() -> Vec<(f64, f64)> {
    MAP_CALLS.with(|v| v.borrow_mut().drain(..).collect())
}

fn array_length(arr: *mut ArrayHeader) -> u32 {
    unsafe { (*arr).length }
}

fn element_bits(arr: *mut ArrayHeader, index: u32) -> u64 {
    crate::array::js_array_get_f64(arr, index).to_bits()
}

/// The bytes of an element expected to be a one-byte ASCII string.
fn element_bytes(arr: *mut ArrayHeader, index: u32) -> Vec<u8> {
    let value = crate::array::js_array_get_f64(arr, index);
    let jsv = JSValue::from_bits(value.to_bits());
    assert!(jsv.is_any_string(), "element {index} is not a string");
    let sptr = crate::value::js_get_string_pointer_unified(value) as *const crate::string::StringHeader;
    unsafe {
        let len = (*sptr).byte_len as usize;
        let data = (sptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

// ---------------------------------------------------------------------------
// Bullet 1: unpatched primitives take the array-like branch (length 0) and
// never reinterpret their bits as a heap address.
// ---------------------------------------------------------------------------

#[test]
fn unpatched_number_yields_an_empty_array() {
    let _scope = TestScope::new();
    let arr = js_array_from_value(5.0);
    assert!(!arr.is_null());
    assert_eq!(array_length(arr), 0);
}

#[test]
fn unpatched_boolean_yields_an_empty_array() {
    let _scope = TestScope::new();
    let arr = js_array_from_value(f64::from_bits(crate::value::TAG_TRUE));
    assert!(!arr.is_null());
    assert_eq!(array_length(arr), 0);
}

#[test]
fn unpatched_symbol_yields_an_empty_array() {
    let _scope = TestScope::new();
    let desc = crate::string::js_string_from_bytes(b"s".as_ptr(), 1);
    let sym = unsafe { crate::symbol::alloc_symbol(desc, false) };
    let value = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
    let arr = js_array_from_value(value);
    assert!(!arr.is_null());
    assert_eq!(array_length(arr), 0);
}

#[test]
fn unpatched_number_mapped_form_yields_empty_without_calling_mapfn() {
    let _scope = TestScope::new();
    let mapfn = boxed_map_fn_times10();
    take_map_calls();
    let arr = js_array_from_mapped(5.0, mapfn, f64::from_bits(crate::value::TAG_UNDEFINED));
    assert_eq!(array_length(arr), 0);
    assert!(take_map_calls().is_empty(), "mapFn must not run for a length-0 source");
}

// ---------------------------------------------------------------------------
// Bullet 2: strings keep their code-point path; a patched
// `String.prototype[Symbol.iterator]` is honored.
// ---------------------------------------------------------------------------

#[test]
fn string_source_yields_code_points() {
    let _scope = TestScope::new();
    let s = crate::string::js_string_from_bytes(b"abc".as_ptr(), 3);
    let value = f64::from_bits(JSValue::string_ptr(s).bits());
    let arr = js_array_from_value(value);
    assert_eq!(array_length(arr), 3);
    assert_eq!(element_bytes(arr, 0), b"a");
    assert_eq!(element_bytes(arr, 1), b"b");
    assert_eq!(element_bytes(arr, 2), b"c");
}

#[test]
fn patched_string_iterator_next_is_honored() {
    let _scope = TestScope::new();
    // Perry's supported patch surface for the string family: the escape
    // (getPrototypeOf on an iterator) arms STRING_ITERATION_NOT_PRISTINE, then
    // a replaced `%StringIteratorPrototype%.next` must be honoured. The
    // built-in lock refuses user writes, so the patch goes through the
    // runtime's own install escape, exactly like the #9846 tests.
    let s = crate::string::js_string_from_bytes(b"abc".as_ptr(), 3);
    let iter = crate::string::string_values_iter(s);
    let proto = crate::object::js_object_get_prototype_of(iter);
    assert!(
        crate::object::iterator_prototypes::string_iteration_not_pristine(),
        "premise: the escape must arm the not-pristine signal"
    );
    let proto_obj = JSValue::from_bits(proto.to_bits()).as_pointer::<crate::object::ObjectHeader>() as *mut crate::object::ObjectHeader;
    let key = crate::string::js_string_from_bytes(b"next".as_ptr(), 4);
    let original_next = crate::object::js_object_get_field_by_name(proto_obj, key).bits();
    // A step function, not the iterator factory: `next` yields 40, 41, done.
    NEXT_CALLS.with(|c| c.set(0));
    let next = js_closure_alloc(crate::fn_info!(patched_next, 0), 0);
    let method = f64::from_bits(JSValue::pointer(next as *const u8).bits());
    crate::object::own_override::as_builtin_definition(|| unsafe {
        set_field(proto_obj, b"next", method)
    });
    let value = f64::from_bits(JSValue::string_ptr(s).bits());
    let arr = js_array_from_value(value);
    assert_eq!(
        array_length(arr),
        2,
        "the patched %StringIteratorPrototype%.next must drive Array.from"
    );
    assert_eq!(element_bits(arr, 0), 40.0f64.to_bits());
    assert_eq!(element_bits(arr, 1), 41.0f64.to_bits());
    crate::object::own_override::as_builtin_definition(|| unsafe {
        set_field(proto_obj, b"next", f64::from_bits(original_next))
    });
}

// ---------------------------------------------------------------------------
// Bullet 3: a patched `Number.prototype[Symbol.iterator]` drives the
// non-mapped `Array.from(5)`.
// ---------------------------------------------------------------------------

#[test]
fn patched_number_prototype_iterator_drives_array_from() {
    let _scope = TestScope::new();
    let method = boxed_two_value_iterator();
    unsafe { patch_prototype_iterator(number_prototype_value(), method) };
    let arr = js_array_from_value(5.0);
    assert_eq!(array_length(arr), 2, "the patched iterator must drive Array.from(5)");
    assert_eq!(element_bits(arr, 0), 40.0f64.to_bits());
    assert_eq!(element_bits(arr, 1), 41.0f64.to_bits());
    unsafe { patch_prototype_iterator(number_prototype_value(), f64::from_bits(crate::value::TAG_UNDEFINED)) };
}

// ---------------------------------------------------------------------------
// Bullet 4: the mapped form applies `mapFn` with the right index, both on the
// patched-primitive iterator path and on the string path.
// ---------------------------------------------------------------------------

#[test]
fn patched_number_iterator_with_mapfn_maps_with_the_right_index() {
    let _scope = TestScope::new();
    let method = boxed_two_value_iterator();
    let mapfn = boxed_map_fn_times10();
    unsafe { patch_prototype_iterator(number_prototype_value(), method) };
    let arr = js_array_from_mapped(5.0, mapfn, f64::from_bits(crate::value::TAG_UNDEFINED));
    assert_eq!(array_length(arr), 2);
    assert_eq!(element_bits(arr, 0), 400.0f64.to_bits());
    assert_eq!(element_bits(arr, 1), 411.0f64.to_bits());
    assert_eq!(take_map_calls(), vec![(40.0, 0.0), (41.0, 1.0)]);
    unsafe { patch_prototype_iterator(number_prototype_value(), f64::from_bits(crate::value::TAG_UNDEFINED)) };
}

#[test]
fn string_source_mapfn_still_receives_the_right_index() {
    let _scope = TestScope::new();
    let mapfn = boxed_map_fn_record_index();
    let s = crate::string::js_string_from_bytes(b"ab".as_ptr(), 2);
    let value = f64::from_bits(JSValue::string_ptr(s).bits());
    let arr = js_array_from_mapped(value, mapfn, f64::from_bits(crate::value::TAG_UNDEFINED));
    assert_eq!(array_length(arr), 2);
    assert_eq!(element_bytes(arr, 0), b"a");
    assert_eq!(element_bytes(arr, 1), b"b");
    let calls = take_map_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].1, 0.0);
    assert_eq!(calls[1].1, 1.0);
}
