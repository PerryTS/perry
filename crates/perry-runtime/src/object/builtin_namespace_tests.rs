use super::*;

fn ns(name: &str) -> usize {
    crate::codegen_abi::builtin_namespace_family(name).expect("a builtin namespace") - FIRST
}

/// This thread's realm global, populated and sealed.
fn global() -> *mut ObjectHeader {
    let g = crate::object::js_get_global_this();
    (g.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader
}

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

unsafe fn member_holder(name: &str) -> *mut ObjectHeader {
    let value = crate::object::js_object_get_field_by_name_f64(global(), key(name));
    holder_of_value(value.to_bits())
}

#[test]
fn sealing_publishes_every_namespace_holder_pristine() {
    let _g = global();
    for name in crate::codegen_abi::BUILTIN_NAMESPACES {
        let i = ns(name);
        assert!(pristine(i), "{name} starts pristine");
        assert!(holder_is_live(holder(i)), "{name} has a published holder");
    }
    // SAFETY: holders of this thread's realm.
    unsafe {
        assert_eq!(
            member_holder("Math") as *const ObjectHeader,
            holder(ns("Math"))
        );
        assert_eq!(
            member_holder("Object") as *const ObjectHeader,
            holder(ns("Object"))
        );
    }
}

#[test]
fn a_member_write_withdraws_only_its_namespace() {
    let _g = global();
    // SAFETY: the realm's Math object.
    unsafe {
        let math = member_holder("Math");
        crate::object::js_object_set_field_by_name(math, key("added"), 1.0);
    }
    assert!(!pristine(ns("Math")));
    assert!(pristine(ns("JSON")) && pristine(ns("Object")));
}

#[test]
fn a_constructor_static_add_withdraws_its_namespace() {
    let g = global();
    // SAFETY: the realm's Object constructor.
    {
        let ctor = crate::object::js_object_get_field_by_name_f64(g, key("Object"));
        let addr = (ctor.to_bits() & crate::value::POINTER_MASK) as usize;
        crate::closure::closure_set_dynamic_prop(addr, "added", 1.0);
    }
    assert!(!pristine(ns("Object")));
    assert!(pristine(ns("Array")));
}

#[test]
fn rebinding_a_namespace_on_the_global_withdraws_it_and_nothing_else() {
    let g = global();
    // SAFETY: the realm global.
    {
        crate::object::js_object_set_field_by_name(g, key("someProgramGlobal"), 2.0);
        assert!(pristine(ns("JSON")), "a new global key rebinds nothing");
        crate::object::js_object_set_field_by_name(g, key("JSON"), 3.0);
    }
    assert!(!pristine(ns("JSON")));
    assert!(pristine(ns("Math")) && pristine(ns("Reflect")));
}

#[test]
fn deleting_a_namespace_binding_withdraws_it() {
    let g = global();
    // SAFETY: the realm global.
    {
        crate::object::js_object_delete_field(g, key("Reflect"));
    }
    assert!(!pristine(ns("Reflect")));
    assert!(pristine(ns("Math")));
}

/// DEC 255: every writer that rebinds a namespace on the global object goes
/// through the global's store funnel, so a compiled `Object.assign(globalThis,
/// shims)` or `Object.defineProperty(globalThis, k, ...)` withdraws exactly the
/// namespaces it names (the guarded call's generic arm then reads the binding).
#[test]
fn assign_and_define_onto_the_global_withdraw_the_named_namespace() {
    let g = global();
    let gv = crate::value::js_nanbox_pointer(g as i64);
    // SAFETY: the realm global and fresh ordinary objects of this thread.
    unsafe {
        let shims = crate::object::js_object_alloc(0, 2);
        crate::object::js_object_set_field_by_name(shims, key("someShim"), 1.0);
        crate::object::js_object_assign_one(gv, crate::value::js_nanbox_pointer(shims as i64));
        assert!(pristine(ns("Math")), "a shim under another key rebinds nothing");
        crate::object::js_object_set_field_by_name(shims, key("Math"), 2.0);
        crate::object::js_object_assign_one(gv, crate::value::js_nanbox_pointer(shims as i64));
        assert!(!pristine(ns("Math")), "assign rebinding Math withdraws it");
        assert!(pristine(ns("Date")) && pristine(ns("JSON")));

        let bag = crate::object::js_object_alloc(0, 2);
        crate::object::js_object_set_field_by_name(bag, key("value"), 3.0);
        crate::object::js_object_set_field_by_name(bag, key("writable"), f64::from_bits(crate::value::TAG_TRUE));
        crate::object::js_object_set_field_by_name(bag, key("configurable"), f64::from_bits(crate::value::TAG_TRUE));
        let k = f64::from_bits(crate::value::JSValue::string_ptr(key("Date")).bits());
        crate::object::js_object_define_property(
            gv,
            k,
            crate::value::js_nanbox_pointer(bag as i64),
        );
        assert!(!pristine(ns("Date")), "defineProperty rebinding Date withdraws it");
        assert!(pristine(ns("JSON")) && pristine(ns("Reflect")));
    }
}

/// `console` is a native namespace object: a member store through a value
/// (what interpreted code and a store through a parameter both reach) lands
/// in the namespace override table, not in the holder. It must still set the
/// holder's bit, or a guarded `console.log(...)` keeps binding the intrinsic.
#[test]
fn a_console_member_store_through_a_value_withdraws_console() {
    let _g = global();
    assert!(pristine(ns("console")));
    // The realm's console object, written through PutValue.
    let console = crate::object::js_object_get_field_by_name_f64(global(), key("console"));
    let name = crate::value::js_nanbox_string(key("log") as i64);
    crate::proxy::js_put_value_set(console, name, 1.0, console, 0);
    assert!(!pristine(ns("console")));
    assert!(pristine(ns("Math")) && pristine(ns("JSON")));
}

/// An override stored before the realm is sealed has already replaced a
/// member: sealing publishes `console` withdrawn.
#[test]
fn a_console_override_stored_before_the_seal_withdraws_console() {
    crate::object::native_module::native_namespace_prop_override_store("console", "info", 1.0);
    let _g = global();
    assert!(!pristine(ns("console")));
    assert!(pristine(ns("Math")));
}
