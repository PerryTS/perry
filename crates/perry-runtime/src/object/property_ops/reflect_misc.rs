//! Shared ordinary property operations. Proxy dispatch uses installer slots.
use super::*;

pub(crate) fn array_from_args(args: &[f64]) -> f64 {
    let arr = crate::array::js_array_alloc(0);
    let mut a = arr;
    for &arg in args {
        a = crate::array::js_array_push_f64(a, arg);
    }
    f64::from_bits(POINTER_TAG | ((a as u64) & POINTER_MASK))
}

#[no_mangle]
pub extern "C" fn js_reflect_construct(target: f64, args_like: f64, new_target: f64) -> f64 {
    if !is_constructor_function(target) {
        // Node reports the offending value: `1 is not a constructor`.
        return throw_type_error(&format!(
            "{} is not a constructor",
            value_display_string(target)
        ));
    }
    let nt = if new_target.to_bits() == TAG_UNDEFINED {
        target
    } else {
        new_target
    };
    if !is_constructor_function(nt) {
        return throw_type_error(&format!(
            "{} is not a constructor",
            value_display_string(nt)
        ));
    }
    let args = create_list_from_array_like(args_like);
    if lookup(target).is_some() {
        let args_array = array_from_args(&args);
        return proxy_construct(target, args_array, nt);
    }
    let (ptr, n) = if args.is_empty() {
        (std::ptr::null::<f64>(), 0usize)
    } else {
        (args.as_ptr(), args.len())
    };
    unsafe { crate::object::js_new_function_construct_with_new_target(target, ptr, n, nt) }
}

/// `Reflect.ownKeys(target)` (#2763) — returns string own-property names
/// followed by own symbol keys (Node order: integer-index then insertion-order
/// string keys, then symbols). Throws `TypeError` for a non-object target.
///
/// For a proxy, dispatches the `ownKeys` trap (with type/duplicate/invariant
/// validation) via [`proxy_own_keys`].
#[no_mangle]
pub extern "C" fn js_reflect_own_keys(target: f64) -> f64 {
    if lookup(target).is_some() {
        return proxy_own_keys(target);
    }
    let real = target;
    if !reflect_value_is_object(real) {
        return reflect_non_object_typeerror("ownKeys");
    }
    // String own names (this fn already throws for null/undefined; we've
    // validated above for the other primitives).
    let names = crate::object::js_object_get_own_property_names(real);
    let names_ptr = (names.to_bits() & POINTER_MASK) as *mut crate::array::ArrayHeader;
    if names_ptr.is_null() {
        return names;
    }
    // Append own symbol keys (#2763).
    let syms_raw = unsafe { crate::symbol::js_object_get_own_property_symbols(real) };
    let syms_ptr = syms_raw as *const crate::array::ArrayHeader;
    if !syms_ptr.is_null() {
        let sym_count = crate::array::js_array_length(syms_ptr) as usize;
        let mut out = names_ptr;
        for i in 0..sym_count {
            let sym = crate::array::js_array_get(syms_ptr, i as u32);
            out = crate::array::js_array_push_f64(out, f64::from_bits(sym.bits()));
        }
        return f64::from_bits(POINTER_TAG | ((out as u64) & POINTER_MASK));
    }
    names
}

/// `Reflect.apply(fn, thisArg, argumentsList)` (#2767).
///
/// - throws `TypeError` for a non-callable target,
/// - implements `CreateListFromArrayLike(argumentsList)` (throws for a
///   non-object `argumentsList`, reads `0..length` from any array-like),
/// - binds `thisArg` for the call.
///
/// Proxy targets still dispatch to `proxy_apply` (which forwards the
/// already-constructed `args_array`). Proxy `apply` trap result fidelity for
/// an `undefined` trap return is out of scope here — Perry's proxy-apply path
/// keeps a pragmatic fallback (see `proxy_apply`).
#[no_mangle]
pub extern "C" fn js_reflect_apply(f: f64, this_arg: f64, args_array: f64) -> f64 {
    // If `f` is a proxy with apply trap, dispatch through it.
    if lookup(f).is_some() {
        return proxy_apply(f, this_arg, args_array);
    }
    // Non-callable target → TypeError (before evaluating argumentsList,
    // matching Node which reports the function check first).
    if !is_callable(f) {
        return throw_type_error("Reflect.apply target is not a function");
    }
    // Building the list allocates (index keys) and can run user getters, so the
    // callee and the receiver are rooted across it (#10532 review).
    let scope = crate::gc::RuntimeHandleScope::new();
    let callee = scope.root_nanbox_f64(f);
    let receiver = scope.root_nanbox_f64(this_arg);
    let args = create_list_from_array_like(args_array);
    call_with_this_and_args(callee.get_nanbox_f64(), receiver.get_nanbox_f64(), &args)
}

/// `Reflect.defineProperty(obj, key, descriptor)` — returns `false` when the
/// definition cannot be applied (#2758): defining a *new* property on a
/// non-extensible object, or redefining an existing *non-configurable*
/// property. Successful definitions return `true`. For a proxy target, the
/// coerced `defineProperty` trap result is returned.
#[no_mangle]
pub extern "C" fn js_reflect_define_property(obj: f64, key: f64, bag: f64) -> f64 {
    unsafe {
        if !crate::object::object_ops::definition_target_is_object(obj) {
            return reflect_non_object_typeerror("defineProperty");
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        // Public operands can use the admitted legacy raw object ABI. Convert
        // them allocation-free before the collecting ToPropertyKey call.
        let receiver =
            scope.root_nanbox_f64(crate::object::object_ops::normalize_descriptor_operand(obj));
        let bag =
            scope.root_nanbox_f64(crate::object::object_ops::normalize_descriptor_operand(bag));
        let key = scope.root_nanbox_f64(crate::object::js_to_property_key(key));
        let descriptor = crate::object::object_ops::decode_property_descriptor(&scope, &bag);
        nanbox_bool(crate::object::object_ops::define_own_property_decoded(
            &scope,
            &receiver,
            &key,
            &descriptor,
        ))
    }
}

/// `Reflect.getPrototypeOf(obj)` — shares the actual prototype lookup with
/// `Object.getPrototypeOf` (#2757): returns the object's `[[Prototype]]`,
/// including `null` for null-prototype objects, not the object itself.
#[no_mangle]
pub extern "C" fn js_reflect_get_prototype_of(obj: f64) -> f64 {
    // Reflect.getPrototypeOf on a non-object target must throw TypeError (spec
    // step 1). Note `Object.getPrototypeOf` is more lenient (ToObject-coerces
    // primitives), so guard here before delegating. Proxies have a registered
    // entry and are objects, so they pass this check and dispatch below.
    if lookup(obj).is_some() {
        return proxy_get_prototype_of_impl(obj);
    }
    if !reflect_value_is_object(obj) {
        return reflect_non_object_typeerror("getPrototypeOf");
    }
    crate::object::js_object_get_prototype_of(obj)
}

/// `Reflect.isExtensible(target)` — throws a `TypeError` for non-object targets
/// (#2762), otherwise returns the boolean extensibility of the target. For a
/// proxy, dispatches to the `isExtensible` trap when present.
#[no_mangle]
pub extern "C" fn js_reflect_is_extensible(target: f64) -> f64 {
    if lookup(target).is_some() {
        return proxy_is_extensible(target);
    }
    if !reflect_value_is_object(target) {
        return reflect_non_object_typeerror("isExtensible");
    }
    crate::object::js_object_is_extensible(target)
}

/// `Reflect.preventExtensions(target)` — throws a `TypeError` for non-object
/// targets (#2762) and returns a boolean (`true` on success), unlike
/// `Object.preventExtensions` which returns the object. For a proxy, dispatches
/// to the `preventExtensions` trap when present and returns its coerced result.
#[no_mangle]
pub extern "C" fn js_reflect_prevent_extensions(target: f64) -> f64 {
    if lookup(target).is_some() {
        return proxy_prevent_extensions(target);
    }
    if !reflect_value_is_object(target) {
        return reflect_non_object_typeerror("preventExtensions");
    }
    crate::object::js_object_prevent_extensions(target);
    nanbox_bool(true)
}
