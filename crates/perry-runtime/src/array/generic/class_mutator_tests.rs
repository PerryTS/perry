use super::*;
use std::cell::Cell;

thread_local! {
    static LENGTH: Cell<f64> = const { Cell::new(0.0) };
    static GETS: Cell<u32> = const { Cell::new(0) };
    static SETS: Cell<u32> = const { Cell::new(0) };
    static RECEIVER: Cell<u64> = const { Cell::new(0) };
}

extern "C" fn length_getter(this: f64) -> f64 {
    RECEIVER.set(this.to_bits());
    GETS.set(GETS.get() + 1);
    LENGTH.get()
}

extern "C" fn length_setter(this: f64, value: f64) -> f64 {
    RECEIVER.set(this.to_bits());
    SETS.set(SETS.get() + 1);
    LENGTH.set(value);
    undef()
}

#[test]
fn borrowed_class_mutators_read_and_write_length_once() {
    let _lock = crate::gc::global_side_table_test_lock();
    const MUTATOR_TEST_CLASS_ID: u32 = 0x1111_2CA1;
    LENGTH.set(0.0);
    GETS.set(0);
    SETS.set(0);
    unsafe {
        crate::object::js_register_class_id(MUTATOR_TEST_CLASS_ID);
        crate::object::js_register_class_name(MUTATOR_TEST_CLASS_ID, b"BorrowedBag".as_ptr(), 11);
        crate::object::js_register_class_getter(
            MUTATOR_TEST_CLASS_ID as i64,
            b"length".as_ptr(),
            6,
            length_getter as *const () as i64,
        );
        crate::object::js_register_class_setter(
            MUTATOR_TEST_CLASS_ID as i64,
            b"length".as_ptr(),
            6,
            length_setter as *const () as i64,
        );
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    // Match compiled classes: materialize their evaluated prototype before
    // attaching it to the instance, so the registered accessors are observable.
    let class = scope.root_raw_mut_ptr(crate::object::js_object_alloc(MUTATOR_TEST_CLASS_ID, 0));
    let proto = unsafe {
        class.with_mut_ptr::<crate::object::ObjectHeader, _>(|p| {
            crate::object::js_object_mark_class(p as i64);
            crate::object::class_object_prototype_value(p)
        })
    };
    let proto = scope.root_nanbox_f64(f64::from_bits(proto.bits()));
    let instance = scope.root_raw_mut_ptr(crate::object::js_object_alloc(MUTATOR_TEST_CLASS_ID, 0));
    instance.with_mut_ptr::<crate::object::ObjectHeader, _>(|p| {
        crate::object::prototype_chain::object_link_class_evaluation_prototype(
            p as usize,
            proto.get_nanbox_f64().to_bits(),
        );
    });
    let receiver = || {
        instance.with_mut_ptr::<crate::object::ObjectHeader, _>(|p| {
            crate::value::js_nanbox_pointer(p as i64)
        })
    };
    let values = [11.0, 22.0];
    assert_eq!(
        array_proto_mutator(receiver(), "push", values.as_ptr(), 2),
        2.0
    );
    assert_eq!((GETS.get(), SETS.get(), LENGTH.get()), (1, 1, 2.0));
    assert_eq!(RECEIVER.get(), receiver().to_bits());
    assert_eq!(al_get(receiver(), 0), 11.0);
    assert_eq!(al_get(receiver(), 1), 22.0);
    assert_eq!(array_proto_mutator(receiver(), "pop", ptr::null(), 0), 22.0);
    assert_eq!((GETS.get(), SETS.get(), LENGTH.get()), (2, 2, 1.0));
    assert_eq!(al_get(receiver(), 1).to_bits(), TAG_UNDEFINED);
    assert_eq!(
        array_proto_mutator(receiver(), "unshift", values.as_ptr().wrapping_add(1), 1),
        2.0
    );
    assert_eq!((GETS.get(), SETS.get(), LENGTH.get()), (3, 3, 2.0));
    assert_eq!(al_get(receiver(), 0), 22.0);
    assert_eq!(al_get(receiver(), 1), 11.0);
    assert_eq!(
        array_proto_mutator(receiver(), "shift", ptr::null(), 0),
        22.0
    );
    assert_eq!((GETS.get(), SETS.get(), LENGTH.get()), (4, 4, 1.0));
    assert_eq!(al_get(receiver(), 0), 11.0);
    // Ordinary method-name dispatch must still defer to the class's methods.
    assert!(try_object_arraylike_mutator(receiver(), "push", values.as_ptr(), 2).is_none());
    assert_eq!((GETS.get(), SETS.get()), (4, 4));
}
