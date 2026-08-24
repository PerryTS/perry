use super::*;

pub(super) fn install_bound_instance_function(
    obj: *mut ObjectHeader,
    name: &str,
    func_ptr: *const u8,
    arity: u32,
) -> *mut ClosureHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let closure = crate::closure::js_closure_alloc(func_ptr, 1);
    if closure.is_null() {
        return closure;
    }
    let closure = scope.root_raw_mut_ptr(closure);
    crate::closure::js_register_closure_arity(func_ptr, arity);
    closure.with_mut_ptr(|closure| {
        obj.with_mut_ptr(|obj: *mut ObjectHeader| {
            crate::closure::js_closure_set_capture_f64(closure, 0, js_nanbox_pointer(obj as i64))
        })
    });
    closure.with_mut_ptr(|closure| crate::object::set_bound_native_closure_name(closure, name));
    closure.with_mut_ptr::<ClosureHeader, _>(|closure| {
        crate::object::set_builtin_closure_length(closure as usize, arity)
    });
    // A bound Intl instance method (`nf.format`, `nf.resolvedOptions`, …) is a
    // built-in non-constructor function: it has NO `[[Construct]]` and therefore
    // no own `prototype` property (ECMA-262 §17 — built-in functions that aren't
    // constructors don't get the auto-created `.prototype`). Flag it so
    // `function_would_have_own_prototype` / the `new` path treat it like any
    // other builtin (`Math.max`), matching `format-function-builtin.js`.
    closure.with_mut_ptr::<ClosureHeader, _>(|closure| {
        crate::object::set_builtin_closure_non_constructable(closure as usize)
    });
    closure.with_mut_ptr(|closure: *mut ClosureHeader| {
        crate::object::set_builtin_property_attrs(
            closure as usize,
            "name".to_string(),
            PropertyAttrs::new(false, false, true),
        )
    });
    closure.with_mut_ptr(|closure: *mut ClosureHeader| {
        crate::object::set_builtin_property_attrs(
            closure as usize,
            "length".to_string(),
            PropertyAttrs::new(false, false, true),
        )
    });
    let closure_value =
        closure.with_mut_ptr(|closure: *mut ClosureHeader| js_nanbox_pointer(closure as i64));
    obj.with_mut_ptr(|obj| set_field(obj, name, closure_value));
    obj.with_mut_ptr(|obj| set_builtin_attrs(obj, name, PropertyAttrs::new(true, false, true)));
    closure.with_mut_ptr(|closure| closure)
}

pub(super) fn install_bound_instance_function_from_handle(
    obj: &crate::gc::RuntimeHandle<'_>,
    name: &str,
    func_ptr: *const u8,
    arity: u32,
) -> *mut ClosureHeader {
    obj.with_mut_ptr(|obj| install_bound_instance_function(obj, name, func_ptr, arity))
}

pub(super) fn install_function(
    owner: *mut ObjectHeader,
    name: &str,
    func_ptr: *const u8,
    call_arity: u32,
    length: u32,
    has_rest: bool,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(owner);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return undefined();
    }
    let closure = scope.root_raw_mut_ptr(closure);
    if has_rest {
        crate::closure::js_register_closure_rest(func_ptr, call_arity);
    } else {
        crate::closure::js_register_closure_arity(func_ptr, call_arity);
    }
    closure.with_mut_ptr(|closure| crate::object::set_bound_native_closure_name(closure, name));
    closure.with_mut_ptr::<ClosureHeader, _>(|closure| {
        crate::object::set_builtin_closure_length(closure as usize, length)
    });
    // Intl prototype methods (`formatToParts`, `resolvedOptions`, …), the static
    // `supportedLocalesOf`, and the this-based instance methods
    // (`formatRange`/`formatRangeToParts`) installed through here are all
    // built-in non-constructor functions: no `[[Construct]]`, hence no own
    // `prototype` property (`builtin.js` asserts `hasOwnProperty("prototype")`
    // is false and `isConstructor` is false). Flag them like any other builtin.
    closure.with_mut_ptr::<ClosureHeader, _>(|closure| {
        crate::object::set_builtin_closure_non_constructable(closure as usize)
    });
    closure.with_mut_ptr(|closure: *mut ClosureHeader| {
        crate::object::set_builtin_property_attrs(
            closure as usize,
            "name".to_string(),
            PropertyAttrs::new(false, false, true),
        )
    });
    closure.with_mut_ptr(|closure: *mut ClosureHeader| {
        crate::object::set_builtin_property_attrs(
            closure as usize,
            "length".to_string(),
            PropertyAttrs::new(false, false, true),
        )
    });
    let value =
        closure.with_mut_ptr(|closure: *mut ClosureHeader| js_nanbox_pointer(closure as i64));
    owner.with_mut_ptr(|owner| set_field(owner, name, value));
    owner.with_mut_ptr(|owner| {
        set_builtin_attrs(owner, name, PropertyAttrs::new(true, false, true))
    });
    value
}

pub(super) fn install_function_from_handle(
    owner: &crate::gc::RuntimeHandle<'_>,
    name: &str,
    func_ptr: *const u8,
    call_arity: u32,
    length: u32,
    has_rest: bool,
) -> f64 {
    owner
        .with_mut_ptr(|owner| install_function(owner, name, func_ptr, call_arity, length, has_rest))
}
