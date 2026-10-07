//! `Function.prototype.call` / `.apply` as values brand-check their `this`:
//! `Function.prototype.apply.call(undefined, …)` reaches the thunk with the
//! non-callable receiver as `this` and must throw a `TypeError`.

extern "C" fn add(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    a: f64,
    b: f64,
) -> f64 {
    a + b
}

/// The thrown value's error kind, or `None` for a normal return.
fn thrown_error_kind(f: impl FnOnce() -> f64) -> Option<u32> {
    let thrown = crate::exception::catch_js_throw(f).err()?;
    let err = crate::value::js_nanbox_get_pointer(thrown) as *const crate::error::ErrorHeader;
    // SAFETY: the thunks throw only `js_typeerror_new` errors.
    Some(unsafe { (*err).error_kind })
}

#[test]
fn call_and_apply_thunks_reject_a_primitive_this_and_still_call_a_function() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let callee = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(add, 2; with_declared(2)), 0) as i64,
    ));
    let args = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(2));
    args.with_mut_ptr(|ptr| crate::array::js_array_set_f64(ptr, 0, 3.0));
    args.with_mut_ptr(|ptr| crate::array::js_array_set_f64(ptr, 1, 4.0));
    let args_value = || {
        args.with_mut_ptr(|p: *mut crate::array::ArrayHeader| {
            crate::value::js_nanbox_pointer(p as i64)
        })
    };
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let this_of = |v: f64| crate::closure::JsThis::from_f64(v);
    let null = std::ptr::null();

    let mut kinds = Vec::new();
    for receiver in [
        undefined,
        f64::from_bits(crate::value::TAG_NULL),
        42.0,
        f64::from_bits(crate::value::TAG_TRUE),
    ] {
        kinds.push(thrown_error_kind(|| {
            super::function_prototype_call_thunk(null, this_of(receiver), undefined, args_value())
        }));
        kinds.push(thrown_error_kind(|| {
            super::function_prototype_apply_thunk(null, this_of(receiver), undefined, args_value())
        }));
    }
    let called = (
        super::function_prototype_call_thunk(
            null,
            this_of(callee.get_nanbox_f64()),
            undefined,
            args_value(),
        ),
        super::function_prototype_apply_thunk(
            null,
            this_of(callee.get_nanbox_f64()),
            undefined,
            args_value(),
        ),
    );
    assert_eq!(
        (kinds, called),
        (
            vec![Some(crate::error::ERROR_KIND_TYPE_ERROR); 8],
            (7.0, 7.0)
        )
    );
}
