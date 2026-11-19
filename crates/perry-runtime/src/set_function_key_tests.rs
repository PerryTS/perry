//! DESIGN_FR step 1: a function used as a Set key keeps its identity in the
//! meta of its key owner (today the bag; `key_owner::meta_for`).
use crate::closure::ClosureHeader;

extern "C" fn empty_body(_closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn function() -> f64 {
    let c = crate::closure::js_closure_alloc(crate::fn_info!(empty_body, 0; with_declared(0)), 0);
    crate::value::js_nanbox_pointer(c as i64)
}

#[test]
fn a_function_key_owns_its_identity() {
    let _global = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let (f, g) = (function(), function());
    let addr = |v: f64| (v.to_bits() & crate::value::POINTER_MASK) as usize;
    assert!(
        unsafe { super::index::key_identity(addr(f), false) }.is_none(),
        "premise: a fresh function has no identity yet"
    );
    let set = super::js_set_alloc(4);
    let set = super::js_set_add(set, f);
    assert_eq!(super::js_set_has(set, f), 1);
    assert_eq!(
        super::js_set_has(set, g),
        0,
        "another function is another key"
    );
    assert!(
        unsafe { super::index::key_identity(addr(f), false) }.is_some(),
        "the add gave the function owner an identity (meta_for creates the meta)"
    );
    assert!(
        unsafe { super::index::key_identity(addr(g), false) }.is_none(),
        "a has() never creates one"
    );
}
