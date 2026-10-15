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
        crate::object::js_object_set_field_by_name(math, key("max"), 1.0);
    }
    assert!(!pristine(ns("Math")));
    assert!(pristine(ns("JSON")) && pristine(ns("Object")));
}

#[test]
fn a_constructor_static_delete_withdraws_its_namespace() {
    let g = global();
    // SAFETY: the realm's Object constructor.
    unsafe {
        let ctor = crate::object::js_object_get_field_by_name_f64(g, key("Object"));
        let addr = (ctor.to_bits() & crate::value::POINTER_MASK) as usize;
        assert!(crate::closure::closure_delete_own_dynamic_prop(
            addr, "keys"
        ));
    }
    assert!(!pristine(ns("Object")));
    assert!(pristine(ns("Array")));
}

#[test]
fn rebinding_a_namespace_on_the_global_withdraws_it_and_nothing_else() {
    let g = global();
    // SAFETY: the realm global.
    unsafe {
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
    unsafe {
        crate::object::js_object_delete_field(g, key("Reflect"));
    }
    assert!(!pristine(ns("Reflect")));
    assert!(pristine(ns("Math")));
}
