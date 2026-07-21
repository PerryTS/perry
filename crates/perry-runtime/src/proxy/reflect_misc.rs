//! Reflect.* entry points + proxy [[GetPrototypeOf]] — extracted from
//! `proxy.rs` to keep it under the 2000-line gate (split-large-files
//! recipe). Pure relocation; `use super::*` preserves visibility of the
//! parent's private registry helpers.

use super::*;

pub(super) fn array_from_args(args: &[f64]) -> f64 {
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
        return js_proxy_construct(target, args_array, nt);
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
/// validation) via [`js_proxy_own_keys`].
#[no_mangle]
pub extern "C" fn js_reflect_own_keys(target: f64) -> f64 {
    if lookup(target).is_some() {
        return own_keys::js_proxy_own_keys(target);
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
/// Proxy targets still dispatch to `js_proxy_apply` (which forwards the
/// already-constructed `args_array`). Proxy `apply` trap result fidelity for
/// an `undefined` trap return is out of scope here — Perry's proxy-apply path
/// keeps a pragmatic fallback (see `js_proxy_apply`).
#[no_mangle]
pub extern "C" fn js_reflect_apply(f: f64, this_arg: f64, args_array: f64) -> f64 {
    // If `f` is a proxy with apply trap, dispatch through it.
    if lookup(f).is_some() {
        return js_proxy_apply(f, this_arg, args_array);
    }
    // Non-callable target → TypeError (before evaluating argumentsList,
    // matching Node which reports the function check first).
    if !is_callable(f) {
        return throw_type_error("Reflect.apply target is not a function");
    }
    let args = create_list_from_array_like(args_array);
    call_with_this_and_args(f, this_arg, &args)
}

/// `Reflect.defineProperty(obj, key, descriptor)` — returns `false` when the
/// definition cannot be applied (#2758): defining a *new* property on a
/// non-extensible object, or redefining an existing *non-configurable*
/// property. Successful definitions return `true`. For a proxy target, the
/// coerced `defineProperty` trap result is returned.
#[no_mangle]
pub extern "C" fn js_reflect_define_property(obj: f64, key: f64, descriptor: f64) -> f64 {
    if lookup(obj).is_some() {
        let id = lookup(obj).unwrap();
        let (target, handler, revoked) = PROXIES.with(|p| {
            p.borrow()
                .get(id as usize)
                .and_then(|o| o.as_ref())
                .map(|e| (e.target, e.handler, e.revoked))
                .unwrap_or((
                    f64::from_bits(TAG_UNDEFINED),
                    f64::from_bits(TAG_UNDEFINED),
                    false,
                ))
        });
        if revoked {
            return revoked_return();
        }
        let trap = handler_trap(handler, "defineProperty");
        if is_callable(trap) {
            let scope = crate::gc::RuntimeHandleScope::new();
            let target_h = scope.root_nanbox_f64(target);
            let key_h = scope.root_nanbox_f64(key);
            let desc_h = scope.root_nanbox_f64(descriptor);
            let trap_result = call_trap(
                handler,
                trap,
                &[
                    target_h.get_nanbox_f64(),
                    key_h.get_nanbox_f64(),
                    desc_h.get_nanbox_f64(),
                ],
            );
            if crate::value::js_is_truthy(trap_result) == 0 {
                return nanbox_bool(false);
            }
            invariants::enforce_define_property_invariant(
                target_h.get_nanbox_f64(),
                key_h.get_nanbox_f64(),
                desc_h.get_nanbox_f64(),
            );
            return nanbox_bool(true);
        }
        // No trap — define on the underlying target. When the target is itself
        // a Proxy, recurse through the proxy dispatch rather than the ordinary
        // path, which would deref the fake pointer.
        if lookup(target).is_some() {
            return js_reflect_define_property(target, key, descriptor);
        }
        return crate::object::reflect_define_property(target, key, descriptor);
    }
    // ECMA-262 28.1.3: Reflect.defineProperty throws when target is not an
    // Object (a Symbol / BigInt primitive slips past the heap-pointer probe).
    if !reflect_value_is_object(obj) {
        return reflect_non_object_typeerror("defineProperty");
    }
    crate::object::reflect_define_property(obj, key, descriptor)
}

/// `[[GetPrototypeOf]]` for a Proxy: invoke the handler's `getPrototypeOf`
/// trap when present, otherwise forward to the TARGET's `[[Prototype]]`. A
/// Proxy itself is a small registered id (not a heap object), so the generic
/// `js_object_get_prototype_of` would mis-read it and return `null` — which
/// broke `Object.getPrototypeOf(proxy).constructor` (drizzle aliases columns as
/// `new Proxy(column, …)` and its `is(value, type)` reads
/// `getPrototypeOf(value).constructor`, crashing on `null.constructor`).
/// Callers must have already confirmed `obj` is a registered proxy.
pub(crate) fn js_proxy_get_prototype_of(obj: f64) -> f64 {
    if lookup(obj).is_none() {
        return crate::object::js_object_get_prototype_of(obj);
    }
    proxy_get_prototype_of_impl(obj)
}

/// Shared Proxy `[[GetPrototypeOf]]` (ECMA-262 §10.5.1): dispatch the trap
/// bound to the handler, validate the result is an Object or `null`, and (when
/// the target is non-extensible) enforce that the trap result matches the
/// target's actual prototype. Used by both `Object.getPrototypeOf(proxy)` and
/// `Reflect.getPrototypeOf(proxy)` so they validate identically.
pub(super) fn proxy_get_prototype_of_impl(obj: f64) -> f64 {
    let Some(id) = lookup(obj) else {
        return crate::object::js_object_get_prototype_of(obj);
    };
    let (target, handler, revoked) = PROXIES.with(|p| {
        p.borrow()
            .get(id as usize)
            .and_then(|o| o.as_ref())
            .map(|e| (e.target, e.handler, e.revoked))
            .unwrap_or((
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
                false,
            ))
    });
    if revoked {
        return revoked_return();
    }
    let trap = handler_trap(handler, "getPrototypeOf");
    let trap_bits = trap.to_bits();
    if trap_bits == TAG_UNDEFINED || trap_bits == TAG_NULL {
        return reflect_target_get_prototype_of(target);
    }
    if !is_callable_function(trap) {
        return throw_type_error("proxy getPrototypeOf trap is not a function");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_h = scope.root_nanbox_f64(target);
    let result = call_trap(handler, trap, &[target_h.get_nanbox_f64()]);
    let result_bits = result.to_bits();
    if result_bits != TAG_NULL && !reflect_value_is_object(result) {
        return throw_type_error("proxy getPrototypeOf trap returned non-object");
    }
    let target = target_h.get_nanbox_f64();
    if crate::object::obj_value_no_extend(target) {
        let actual = reflect_target_get_prototype_of(target);
        if actual.to_bits() != result_bits {
            return throw_type_error("proxy getPrototypeOf trap violates target invariant");
        }
    }
    result
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
    if let Some(id) = lookup(target) {
        let (inner, handler, revoked) = PROXIES.with(|p| {
            p.borrow()
                .get(id as usize)
                .and_then(|o| o.as_ref())
                .map(|e| (e.target, e.handler, e.revoked))
                .unwrap_or((
                    f64::from_bits(TAG_UNDEFINED),
                    f64::from_bits(TAG_UNDEFINED),
                    false,
                ))
        });
        if revoked {
            return revoked_return();
        }
        let trap = handler_trap(handler, "isExtensible");
        if is_callable(trap) {
            let scope = crate::gc::RuntimeHandleScope::new();
            let inner_h = scope.root_nanbox_f64(inner);
            let trap_result = call_trap(handler, trap, &[inner_h.get_nanbox_f64()]);
            let booleanized = crate::value::js_is_truthy(trap_result) != 0;
            // Invariant: the trap result must equal the target's actual
            // extensibility.
            let target_ext = crate::value::js_is_truthy(crate::object::js_object_is_extensible(
                inner_h.get_nanbox_f64(),
            )) != 0;
            if booleanized != target_ext {
                return throw_type_error(
                    "proxy isExtensible trap result does not match target extensibility",
                );
            }
            return nanbox_bool(booleanized);
        }
        return crate::object::js_object_is_extensible(inner);
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
    if let Some(id) = lookup(target) {
        let (inner, handler, revoked) = PROXIES.with(|p| {
            p.borrow()
                .get(id as usize)
                .and_then(|o| o.as_ref())
                .map(|e| (e.target, e.handler, e.revoked))
                .unwrap_or((
                    f64::from_bits(TAG_UNDEFINED),
                    f64::from_bits(TAG_UNDEFINED),
                    false,
                ))
        });
        if revoked {
            return revoked_return();
        }
        let trap = handler_trap(handler, "preventExtensions");
        if is_callable(trap) {
            let scope = crate::gc::RuntimeHandleScope::new();
            let inner_h = scope.root_nanbox_f64(inner);
            let trap_result = call_trap(handler, trap, &[inner_h.get_nanbox_f64()]);
            let booleanized = crate::value::js_is_truthy(trap_result) != 0;
            // Invariant: a `true` result requires the target to be non-extensible.
            if booleanized {
                let target_ext = crate::value::js_is_truthy(
                    crate::object::js_object_is_extensible(inner_h.get_nanbox_f64()),
                ) != 0;
                if target_ext {
                    return throw_type_error(
                        "proxy preventExtensions trap returned true but target is extensible",
                    );
                }
            }
            return nanbox_bool(booleanized);
        }
        crate::object::js_object_prevent_extensions(inner);
        return nanbox_bool(true);
    }
    if !reflect_value_is_object(target) {
        return reflect_non_object_typeerror("preventExtensions");
    }
    crate::object::js_object_prevent_extensions(target);
    nanbox_bool(true)
}

/// Native trampoline backing the `revoke` function returned by
/// `Proxy.revocable`. The closure captures the proxy value in capture slot 0;
/// invoking it revokes that specific proxy. Idempotent — revoking an
/// already-revoked proxy is a no-op (Node's `revoke()` is idempotent). (#2846)
pub(super) extern "C" fn proxy_revoke_trampoline(
    closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let proxy = crate::closure::js_closure_get_capture_f64(closure, 0);
    js_proxy_revoke(proxy);
    f64::from_bits(TAG_UNDEFINED)
}
