use super::*;
use crate::closure::{ClosureHeader, JsThis};
use crate::value::JSValue;
use std::sync::atomic::Ordering;

// These fixtures expose an intrinsic iterator prototype and assign a custom
// array prototype. Restore test-only process globals once their arrays die,
// following dyn_eval::tests::ArrayPrototypeLatchGuard.
struct ArrayGlobalsGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    static_proto: bool,
    index: u8,
}

impl ArrayGlobalsGuard {
    fn new() -> Self {
        Self {
            _lock: crate::typed_feedback::typed_feedback_test_lock(),
            static_proto: crate::object::prototype_chain::array_static_proto_recorded(),
            index: crate::array::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(Ordering::Relaxed),
        }
    }
}

impl Drop for ArrayGlobalsGuard {
    fn drop(&mut self) {
        crate::object::prototype_chain::test_swap_array_static_proto_recorded(self.static_proto);
        crate::array::test_swap_array_index_fast_path_invalidated(self.index);
    }
}

thread_local! {
    static FACTORY_CALLS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

extern "C" fn factory(_c: *const ClosureHeader, this: JsThis) -> f64 {
    FACTORY_CALLS.with(|calls| calls.set(calls.get() + 1));
    let arr = js_nanbox_get_pointer(this.as_f64()) as *const super::super::ArrayHeader;
    assert_eq!(super::super::js_array_length(arr), 1);
    super::super::array_values_iter(this.as_f64())
}

extern "C" fn getter(c: *const ClosureHeader, this: JsThis) -> f64 {
    let arr = js_nanbox_get_pointer(this.as_f64()) as *const super::super::ArrayHeader;
    assert_eq!(super::super::js_array_length(arr), 1);
    crate::closure::js_closure_get_capture_f64(c, 0)
}

#[test]
fn array_iterator_slot_data_accessor_and_intrinsic_are_distinct() {
    FACTORY_CALLS.with(|calls| calls.set(0));
    let _stable = crate::gc::GcSuppressScope::new();
    unsafe {
        let arr = super::super::js_array_alloc(1);
        let arr = super::super::js_array_push_f64(arr, 17.0);
        let value = crate::value::js_nanbox_pointer(arr as i64);
        let sym = crate::symbol::well_known_symbol("iterator") as usize;
        let method = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
            crate::fn_info!(factory, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE)),
            0,
        ) as i64);
        crate::symbol::js_object_set_symbol_property(
            value,
            crate::value::js_nanbox_pointer(sym as i64),
            method,
        );
        let owner = super::super::array_property_bag(arr);
        let slot = crate::object::shaped_symbols::position(owner, sym).unwrap();
        assert!(
            matches!(iterator_method_at(owner, slot), IteratorMethod::Data(v) if v.to_bits() == method.to_bits())
        );
        crate::object::shapes::learn_object_constfn_lanes(owner, |i, bits| {
            i == slot && bits == method.to_bits()
        });
        assert!(crate::object::shapes::object_shape_record(owner)
            .unwrap()
            .constfn_info(slot)
            .is_some());
        assert!(
            matches!(iterator_method_at(owner, slot), IteratorMethod::Data(v) if v.to_bits() == method.to_bits()),
            "a ConstFn with the wrong body must still call that body"
        );
        let get = crate::closure::js_closure_alloc(crate::fn_info!(getter, 0), 1);
        crate::closure::js_closure_set_capture_f64(get, 0, method);
        crate::object::shaped_symbols::define_accessor(
            arr as usize,
            sym,
            JSValue::pointer(get.cast()).bits(),
            0,
        );
        let owner = super::super::array_property_bag(arr);
        assert!(
            crate::object::shapes::object_shape_record(owner)
                .unwrap()
                .constfn_info(slot)
                .is_none(),
            "accessor publication must revoke the data ConstFn lane"
        );
        assert!(
            matches!(iterator_method_at(owner, slot), IteratorMethod::Getter(v) if v.to_bits() == JSValue::pointer(get.cast()).bits())
        );
        let iter = array_shape_iterator(value);
        assert_eq!(
            FACTORY_CALLS.with(std::cell::Cell::get),
            1,
            "own getter must supply the method exactly once"
        );
        let next = super::super::js_iterator_next_method(iter);
        let mut item = 0.0;
        assert_eq!(super::super::js_iterator_step(iter, next, &mut item), 0);
        assert_eq!(item, 17.0);

        crate::object::shaped_symbols::define_accessor(arr as usize, sym, 0, 0);
        let owner = super::super::array_property_bag(arr);
        assert!(
            matches!(iterator_method_at(owner, slot), IteratorMethod::Getter(v) if v.to_bits() == TAG_UNDEFINED)
        );
    }
}

#[test]
fn array_stack_entry_acquires_once_and_publishes_next() {
    FACTORY_CALLS.with(|calls| calls.set(0));
    let _stable = crate::gc::GcSuppressScope::new();
    unsafe {
        let arr = super::super::js_array_alloc(1);
        let arr = super::super::js_array_push_f64(arr, 17.0);
        let value = crate::value::js_nanbox_pointer(arr as i64);
        let method = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
            crate::fn_info!(factory, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE)),
            0,
        ) as i64);
        crate::symbol::js_object_set_symbol_property(
            value,
            crate::value::js_nanbox_pointer(crate::symbol::well_known_symbol("iterator") as i64),
            method,
        );
        let mut record = super::super::iterator_stack_record::ArrayStackRecord {
            payload: value,
            next: f64::from_bits(TAG_UNDEFINED),
            index: 0.0,
            state: 0.0,
            protocol: 1.0,
            value: f64::from_bits(TAG_UNDEFINED),
        };
        assert_eq!(
            super::super::iterator_step::js_array_record_stack_enter(&mut record, 1, 0),
            1
        );
        assert_eq!(
            FACTORY_CALLS.with(std::cell::Cell::get),
            1,
            "entry must call the own method once"
        );
        assert_ne!(
            record.payload.to_bits(),
            value.to_bits(),
            "entry must acquire the iterator, not leave the source"
        );
        assert_ne!(
            record.next.to_bits(),
            TAG_UNDEFINED,
            "entry must capture next"
        );
        crate::symbol::js_object_set_symbol_property(
            value,
            crate::value::js_nanbox_pointer(crate::symbol::well_known_symbol("iterator") as i64),
            99.0,
        );
        let mut item = 0.0;
        assert_eq!(
            super::super::js_iterator_step(record.payload, record.next, &mut item),
            0
        );
        assert_eq!(item, 17.0);
    }
}
#[test]
fn array_custom_prototype_supplies_the_method_to_both_consumers() {
    let _globals = ArrayGlobalsGuard::new();
    FACTORY_CALLS.with(|calls| calls.set(0));
    let _stable = crate::gc::GcSuppressScope::new();
    unsafe {
        let arr = super::super::js_array_alloc(1);
        let arr = super::super::js_array_push_f64(arr, 17.0);
        let value = crate::value::js_nanbox_pointer(arr as i64);
        let method = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
            crate::fn_info!(factory, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE)),
            0,
        ) as i64);
        let proto = crate::object::js_object_alloc_null_proto(0, 0);
        let proto_value = crate::value::js_nanbox_pointer(proto as i64);
        crate::symbol::js_object_set_symbol_property(
            proto_value,
            crate::value::js_nanbox_pointer(crate::symbol::well_known_symbol("iterator") as i64),
            method,
        );
        crate::object::js_object_set_prototype_of(value, proto_value);
        let iter = array_shape_iterator(value);
        assert_eq!(FACTORY_CALLS.with(std::cell::Cell::get), 1);
        // Exercise the warm resolver too: a cold standalone runtime has no
        // default prototype, so its absent-owner fallback could hide a broken
        // custom-prototype guard by taking ordinary [[Get]] anyway.
        let default_proto = super::super::js_array_alloc(0);
        let default_method = crate::closure::js_closure_alloc(
            crate::fn_info!(crate::object::array_prototype_values_thunk, 1;
                with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE)),
            0,
        );
        crate::symbol::js_object_set_symbol_property(
            crate::value::js_nanbox_pointer(default_proto as i64),
            crate::value::js_nanbox_pointer(crate::symbol::well_known_symbol("iterator") as i64),
            crate::value::js_nanbox_pointer(default_method as i64),
        );
        let _warm_iter = array_shape_iterator_resolved(value, default_proto as usize);
        assert_eq!(FACTORY_CALLS.with(std::cell::Cell::get), 2);
        let mut stack = super::super::iterator_stack_record::ArrayStackRecord {
            payload: value,
            next: f64::from_bits(TAG_UNDEFINED),
            index: 0.0,
            state: 0.0,
            protocol: 1.0,
            value: f64::from_bits(TAG_UNDEFINED),
        };
        assert_eq!(
            super::super::iterator_step::js_array_record_stack_enter(&mut stack, 1, 0),
            1
        );
        assert_eq!(FACTORY_CALLS.with(std::cell::Cell::get), 3);
        let mut item = 0.0;
        let next = super::super::js_iterator_next_method(iter);
        assert_eq!(super::super::js_iterator_step(iter, next, &mut item), 0);
        assert_eq!(item, 17.0);
    }
}
thread_local! {
    static NEXT_GETS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}
extern "C" fn next_getter(c: *const ClosureHeader, this: JsThis) -> f64 {
    NEXT_GETS.with(|count| count.set(count.get() + 1));
    unsafe {
        let object = js_nanbox_get_pointer(this.as_f64()) as *const ObjectHeader;
        assert_eq!((*object).class_id, super::super::ARRAY_ITERATOR_CLASS_ID);
    }
    crate::closure::js_closure_get_capture_f64(c, 0)
}

#[test]
fn array_next_shape_read_honours_replacement_accessor_own_and_foreign_edge() {
    let _globals = ArrayGlobalsGuard::new();
    NEXT_GETS.with(|count| count.set(0));
    let _stable = crate::gc::GcSuppressScope::new();
    unsafe {
        let value = crate::value::js_nanbox_pointer(super::super::js_array_alloc(0) as i64);
        let iter = super::super::array_values_iter(value);
        let owner_value = crate::object::js_object_get_prototype_of(iter);
        let owner = js_nanbox_get_pointer(owner_value) as *mut ObjectHeader;
        let key = crate::string::intern_ascii_literal(b"next");
        let key_value = f64::from_bits(JSValue::string_ptr(key as *mut crate::StringHeader).bits());
        let original = super::super::js_iterator_next_method(iter);
        crate::object::js_object_set_field_by_name(owner, key, 17.0);
        assert_eq!(
            super::super::js_iterator_next_method(iter),
            17.0,
            "capture the replacement without testing callability"
        );
        let get = crate::closure::js_closure_alloc(crate::fn_info!(next_getter, 0), 1);
        crate::closure::js_closure_set_capture_f64(get, 0, 19.0);
        crate::object::js_object_define_accessor(
            owner_value,
            key_value,
            crate::value::js_nanbox_pointer(get as i64),
            f64::from_bits(TAG_UNDEFINED),
        );
        assert_eq!(super::super::js_iterator_next_method(iter), 19.0);
        assert_eq!(NEXT_GETS.with(std::cell::Cell::get), 1);
        let own_descriptor = crate::object::js_object_alloc_null_proto(0, 0);
        crate::object::js_object_set_field_by_name(
            own_descriptor,
            crate::string::intern_ascii_literal(b"value"),
            23.0,
        );
        crate::object::js_object_define_property(
            iter,
            key_value,
            crate::value::js_nanbox_pointer(own_descriptor as i64),
        );
        assert_eq!(
            super::super::js_iterator_next_method(iter),
            23.0,
            "own next must shadow the family's accessor"
        );
        let foreign = crate::object::js_object_alloc_null_proto(0, 0);
        crate::object::js_object_set_field_by_name(foreign, key, 31.0);
        let other = super::super::array_values_iter(value);
        crate::object::js_object_set_prototype_of(
            other,
            crate::value::js_nanbox_pointer(foreign as i64),
        );
        assert_eq!(
            super::super::js_iterator_next_method(other),
            31.0,
            "a foreign edge must not read the realm owner"
        );
        let descriptor = crate::object::js_object_alloc_null_proto(0, 0);
        for (name, value) in [
            (b"value".as_slice(), original),
            (
                b"writable".as_slice(),
                f64::from_bits(crate::value::TAG_TRUE),
            ),
            (
                b"configurable".as_slice(),
                f64::from_bits(crate::value::TAG_TRUE),
            ),
        ] {
            crate::object::js_object_set_field_by_name(
                descriptor,
                crate::string::intern_ascii_literal(name),
                value,
            );
        }
        crate::object::js_object_define_property(
            owner_value,
            key_value,
            crate::value::js_nanbox_pointer(descriptor as i64),
        );
    }
}

/// A realm whose `%Array.prototype%` is not materialized yet: no code has
/// reached the prototype, so its iteration member is still the intrinsic and
/// iteration needs no resolver (the materialization memo is primed before any
/// user code, `prime_prototype_addr_cache`).
#[test]
fn unmaterialized_array_prototype_iterates_with_the_intrinsic() {
    unsafe {
        assert!(matches!(
            prototype_iterator_method(0),
            IteratorMethod::Intrinsic
        ));
    }
}

#[test]
fn unresolved_intrinsic_prototype_has_the_declared_default_iterator() {
    // Before a realm is exposed there can be no user write to its slot.
    // Resolving it need not bootstrap the realm just to rediscover the
    // runtime-declared intrinsic.
    unsafe {
        assert!(matches!(
            prototype_iterator_method(0),
            IteratorMethod::Intrinsic
        ));
    }
}

/// Object model step 4: while the array's named holder has no overflow, a
/// delete of its own @@iterator holes the fixed slot and keeps the layout key
/// (`shaped_symbols::delete`). Once the holder has an overflow, the overflow
/// is the order authority: the delete goes through it (the fixed slot still
/// becomes the hole marker, so iteration continues on the prototype), and a
/// redefinition is an overflow entry that iteration reaches through the
/// generic GetMethod. Deleting an absent symbol establishes no overflow.
#[test]
fn own_iterator_delete_and_redefine_through_the_holders_overflow() {
    crate::gc::initialize_runtime_fixture();
    FACTORY_CALLS.with(|calls| calls.set(0));
    let _stable = crate::gc::GcSuppressScope::new();
    unsafe {
        let arr = super::super::js_array_alloc(1);
        let arr = super::super::js_array_push_f64(arr, 17.0);
        let value = crate::value::js_nanbox_pointer(arr as i64);
        let sym = crate::symbol::well_known_symbol("iterator") as usize;
        let sym_value = crate::value::js_nanbox_pointer(sym as i64);
        let method = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
            crate::fn_info!(factory, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE)),
            0,
        ) as i64);
        crate::symbol::js_object_set_symbol_property(value, sym_value, method);
        let owner = super::super::array_property_bag(arr);
        assert_eq!(
            crate::object::shaped_symbols::position(owner, sym),
            Some(crate::object::runtime_layouts::ARRAY_PROTOTYPE_ITERATOR_SLOT),
            "premise: the own method sits in the declared iterator slot"
        );

        // An absent symbol: nothing to delete, and no overflow appears.
        let absent = crate::symbol::js_symbol_new_empty();
        assert!(crate::object::shaped_symbols::delete(
            arr as usize,
            (absent.to_bits() & crate::value::POINTER_MASK) as usize,
        ));
        assert!(!crate::object::overflow::has_overflow(owner));

        assert!(crate::object::overflow::establish_order(owner));
        assert_ne!(
            crate::symbol::js_object_delete_symbol_property(value, sym_value),
            0
        );
        let owner = super::super::array_property_bag(arr);
        assert!(crate::object::overflow::has_overflow(owner));
        assert_eq!(
            iterator_slot_value(
                owner,
                crate::object::runtime_layouts::ARRAY_PROTOTYPE_ITERATOR_SLOT
            ),
            crate::value::TAG_HOLE,
            "the delete holes the fixed slot"
        );
        assert_eq!(
            crate::symbol::js_object_get_symbol_property(value, sym_value).to_bits(),
            crate::symbol::js_object_get_symbol_property(
                crate::value::js_nanbox_pointer(super::super::array_prototype_addr() as i64),
                sym_value,
            )
            .to_bits(),
            "after the delete the method is the prototype's"
        );
        let iter = array_shape_iterator(value);
        let next = super::super::js_iterator_next_method(iter);
        let mut item = 0.0;
        assert_eq!(super::super::js_iterator_step(iter, next, &mut item), 0);
        assert_eq!(item, 17.0);
        assert_eq!(FACTORY_CALLS.with(std::cell::Cell::get), 0);

        crate::symbol::js_object_set_symbol_property(value, sym_value, method);
        assert_eq!(
            crate::symbol::js_object_get_symbol_property(value, sym_value).to_bits(),
            method.to_bits(),
            "the redefinition is an own property again"
        );
        let owner = super::super::array_property_bag(arr);
        assert_eq!(
            iterator_slot_value(
                owner,
                crate::object::runtime_layouts::ARRAY_PROTOTYPE_ITERATOR_SLOT
            ),
            method.to_bits(),
            "the redefinition meets the layout's fixed slot (§3.3)"
        );
        let iter = array_shape_iterator(value);
        assert_eq!(
            FACTORY_CALLS.with(std::cell::Cell::get),
            1,
            "iteration calls the redefined own method"
        );
        let next = super::super::js_iterator_next_method(iter);
        assert_eq!(super::super::js_iterator_step(iter, next, &mut item), 0);
        assert_eq!(item, 17.0);

        // An accessor moves out of the fixed slot into its overflow entry;
        // the hole there is not absence, and iteration calls the getter.
        let get = crate::closure::js_closure_alloc(crate::fn_info!(getter, 0), 1);
        crate::closure::js_closure_set_capture_f64(get, 0, method);
        crate::object::shaped_symbols::define_accessor(
            arr as usize,
            sym,
            JSValue::pointer(get.cast()).bits(),
            0,
        );
        let owner = super::super::array_property_bag(arr);
        assert_eq!(
            iterator_slot_value(
                owner,
                crate::object::runtime_layouts::ARRAY_PROTOTYPE_ITERATOR_SLOT
            ),
            crate::value::TAG_HOLE,
            "premise: the accessor lives in the overflow entry"
        );
        let iter = array_shape_iterator(value);
        assert_eq!(
            FACTORY_CALLS.with(std::cell::Cell::get),
            2,
            "iteration calls the own getter's method"
        );
        let next = super::super::js_iterator_next_method(iter);
        assert_eq!(super::super::js_iterator_step(iter, next, &mut item), 0);
        assert_eq!(item, 17.0);
    }
}
