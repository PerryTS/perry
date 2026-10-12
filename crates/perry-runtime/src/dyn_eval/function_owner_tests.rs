use super::*;
use crate::dyn_eval::*;
fn closure_cell(closure: f64) -> f64 {
    crate::closure::js_closure_get_capture_f64(
        crate::value::js_nanbox_get_pointer(closure) as *const _,
        0,
    )
}
fn call0(function: f64) -> f64 {
    unsafe {
        crate::closure::js_native_call_value(
            function,
            crate::closure::plain_call_receiver(),
            std::ptr::null(),
            0,
        )
    }
}
fn empty() -> InterpFn {
    InterpFn {
        params: Vec::new(),
        body: InterpBody::Block(Vec::new()),
        hoisted_vars: Vec::new(),
        strict: false,
        node_functions: Default::default(),
    }
}
#[test]
fn node_memos_are_owned_by_their_parent_ast() {
    let first_parent = register_fn(empty());
    let second_parent = register_fn(empty());
    let first = node_fn_id(first_parent, 0x1000, empty);
    let second = node_fn_id(second_parent, 0x1000, empty);
    let again = node_fn_id(first_parent, 0x1000, empty);
    assert!(!Rc::ptr_eq(
        &lookup_fn(first).unwrap(),
        &lookup_fn(second).unwrap()
    ));
    assert!(Rc::ptr_eq(
        &lookup_fn(first).unwrap(),
        &lookup_fn(again).unwrap()
    ));
    assert_eq!(
        lookup_fn(first_parent)
            .unwrap()
            .node_functions
            .borrow()
            .len(),
        1
    );
}
#[test]
fn repeated_source_and_nested_functions_share_only_native_asts() {
    let first = dyn_function_from_strings(&["return function inner(){return 42};".into()]);
    let first_root = root_push(first);
    let second = dyn_function_from_strings(&["return function inner(){return 42};".into()]);
    assert_ne!(first.to_bits(), second.to_bits());
    assert!(Rc::ptr_eq(
        &lookup_fn(closure_cell(first)).unwrap(),
        &lookup_fn(closure_cell(second)).unwrap()
    ));
    let nested = call0(root_get(first_root));
    let nested_root = root_push(nested);
    let next = call0(root_get(first_root));
    assert_ne!(nested.to_bits(), next.to_bits());
    assert!(Rc::ptr_eq(
        &lookup_fn(closure_cell(root_get(nested_root))).unwrap(),
        &lookup_fn(closure_cell(next)).unwrap()
    ));
    assert_eq!(call0(next), 42.0);
    roots_truncate(first_root);
}
#[test]
fn throws_release_active_native_ast_pins() {
    let base = roots_len();
    let closure = dyn_function_from_strings(&["throw 7;".into()]);
    assert_eq!(roots_len(), base);
    let outcome = crate::exception::catch_js_throw(|| call0(closure));
    assert!(outcome.is_err());
    assert_eq!(roots_len(), base);
    assert!(ACTIVE_FUNCTIONS.with(|pins| pins.borrow().is_empty()));
    assert_eq!(PIN_TOP.with(Cell::get), 0);
}
#[test]
fn source_cache_keeps_its_entry_and_byte_caps() {
    for i in 0..SOURCE_FN_CACHE_MAX + 2 {
        prepare_function_args(&[format!("return {i};")]);
    }
    assert_eq!(
        SOURCE_FN_CACHE.with(|c| c.borrow().len()),
        SOURCE_FN_CACHE_MAX
    );
    SOURCE_FN_CACHE.with(|c| c.borrow_mut().clear());
    SOURCE_FN_CACHE_BYTES.with(|b| b.set(SOURCE_FN_CACHE_MAX_BYTES));
    prepare_function_args(&["return 17; // byte cap".into()]);
    assert!(SOURCE_FN_CACHE.with(|c| c.borrow().is_empty()));
    SOURCE_FN_CACHE_BYTES.with(|b| b.set(0));
}
