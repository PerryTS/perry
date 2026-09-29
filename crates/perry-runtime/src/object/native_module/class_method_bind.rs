//! Class-method binding: the bound-method closures `obj.method` reads build
//! for class instances (`js_class_method_bind`, its snapshot and by-id
//! forms) and the private-brand-aware builder they share.

use super::*;

/// Build a "bound method" closure for `obj.method` PropertyGet on a known class
/// instance. The captures (instance, method_name_ptr, method_name_len) drive
/// `dispatch_bound_method` (closure.rs), which calls `js_native_call_method`
/// — that resolves the method through `CLASS_VTABLE_REGISTRY` for any class
/// registered by `js_register_class_method` at module init.
///
/// Issue #446: previously a class method reference (`let f = obj.method`,
/// `typeof obj.method`, `arr.map(obj.method)`) silently lowered to the
/// generic property-bag lookup, which doesn't store prototype methods —
/// every such read returned `undefined`, so `typeof obj.method === "undefined"`
/// and a captured method ran no body when invoked.
///
/// Method-name pointer is expected to be stable for the closure's lifetime;
/// codegen emits it from the per-module `.str.N.bytes` rodata global.
#[no_mangle]
pub extern "C" fn js_class_method_bind(
    instance: f64,
    method_name_ptr: *const u8,
    method_name_len: usize,
) -> f64 {
    if !method_name_ptr.is_null() && method_name_len > 0 {
        if let Ok(name) = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(method_name_ptr, method_name_len))
        } {
            if matches!(
                name,
                "append"
                    | "delete"
                    | "entries"
                    | "forEach"
                    | "get"
                    | "getSetCookie"
                    | "has"
                    | "keys"
                    | "set"
                    | "Symbol.iterator"
                    | "@@iterator"
                    | "values"
            ) {
                let bits = instance.to_bits();
                if (bits >> 48) == 0x7FFD {
                    let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
                    if crate::value::addr_class::is_small_handle(id as usize) {
                        if let Some(dispatch) = handle_property_dispatch() {
                            let value = HANDLE_PROPERTY_BIND_REENTRY.with(|guard| {
                                if guard.get() {
                                    None
                                } else {
                                    guard.set(true);
                                    let value =
                                        unsafe { dispatch(id, method_name_ptr, method_name_len) };
                                    guard.set(false);
                                    Some(value)
                                }
                            });
                            if let Some(value) = value {
                                if value.to_bits() != crate::value::TAG_UNDEFINED {
                                    return value;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Method IDENTITY (test262 class/elements): a class method is a single
    // shared function object, so `c.m`, `c2.m` and `C.prototype.m` must all be
    // the IDENTICAL value. Route every user-class method-as-value read through
    // the per-`(owner_class, name)` cached canonical built by
    // `class_prototype_method_value_for_name` instead of minting a fresh
    // per-receiver closure here. The canonical captures the OWNER class's
    // prototype-ref (capture 0); `dispatch_bound_method` recognises that marker
    // and supplies the call-site `this` argument so invocations still see
    // the right receiver — e.g. the `this.m = this.m.bind(this)` idiom rebinds
    // correctly, and a bare `const f = c.m; f()` runs with the spec `this`.
    //
    // Guard against re-entry from `class_prototype_method_value_for_name`
    // itself: it builds the canonical by calling `build_bound_method_closure`
    // directly (NOT this function), so the cache is populated without looping.
    if !method_name_ptr.is_null() && method_name_len > 0 {
        if let Ok(name) = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(method_name_ptr, method_name_len))
        } {
            // #7689: a CONSTRUCTOR class-ref receiver (`const f = C.m`) must
            // never canonicalize to the INSTANCE vtable method of the same
            // name — in JS `C.m` sees only statics (`class C { static lex(){}
            // lex(){} }` has `C.lex` === the static; the instance `lex` lives
            // on `C.prototype`). `class_id_from_method_receiver` treats a
            // class ref like an instance, so marked's `const lexer2 =
            // _Lexer.lex; lexer2(src, opt)` extracted the instance `lex`,
            // whose bare invocation read `this.options` off an unconstructed
            // receiver. Fall through to `build_bound_method_closure`: its
            // call-time dispatch (`js_native_call_method`'s 0x7FFE arm)
            // resolves statics-first for constructor refs. PROTOTYPE refs
            // (`C.prototype.m`) keep the canonical path — the instance method
            // is exactly what they name.
            let receiver_class_ref = class_ref_id(instance);
            let receiver_is_constructor_ref =
                receiver_class_ref.is_some() && class_prototype_ref_id(instance).is_none();
            if !receiver_is_constructor_ref && bound_native_method_length(name).is_none() {
                if let Some(class_id) =
                    class_id_from_method_receiver_known(instance, receiver_class_ref)
                {
                    let private_owner = super::take_private_method_owner_hint(name);
                    if let Some(owner) = private_owner
                        .or_else(|| super::class_registry::method_owner_class_id(class_id, name))
                    {
                        // [[Get]] order: an OWN data property of this name
                        // shadows the prototype method. The ubiquitous
                        // `this.m = this.m.bind(this)` idiom installs an own `m`
                        // (a bound function), so `obj.m` must read that own value
                        // back — not the shared prototype method. Skipping this
                        // both returned the wrong identity (`obj.m ===
                        // C.prototype.m` where Node says false) and looped when
                        // the canonical re-resolved `m` by name. A class
                        // prototype-ref receiver has no own-property bag, so this
                        // check is naturally a no-op there.
                        let recv_jsv = JSValue::from_bits(instance.to_bits());
                        if private_owner.is_none()
                            && recv_jsv.is_pointer()
                            && !super::class_registry::is_registered_class_prototype_object(
                                crate::value::js_nanbox_get_pointer(instance) as usize,
                            )
                        {
                            let obj = recv_jsv.as_pointer::<ObjectHeader>();
                            if crate::value::addr_class::is_above_handle_band(obj as usize) {
                                let key = crate::string::js_string_from_bytes(
                                    method_name_ptr,
                                    method_name_len as u32,
                                );
                                if let Some(own) =
                                    unsafe { super::own_data_field_by_name(obj, key) }
                                {
                                    if own.bits() != crate::value::TAG_UNDEFINED {
                                        return f64::from_bits(own.bits());
                                    }
                                }
                            }
                        }
                        let lexical_owner = private_owner
                            .and_then(|owner| super::current_private_lexical_brand_value(owner));
                        let canonical = lexical_owner
                            .or_else(|| private_evaluation_brand_value(instance))
                            .map(|brand| class_evaluation_method_value_for_name(owner, name, brand))
                            .unwrap_or_else(|| class_prototype_method_value_for_name(owner, name));
                        if canonical.to_bits() != crate::value::TAG_UNDEFINED {
                            return canonical;
                        }
                    }
                }
            }
        }
    }

    build_bound_method_closure(instance, method_name_ptr, method_name_len)
}

/// Perry's intentional `this.method` value-read contract: capture the instance
/// at read time so a later own-property replacement cannot change the method's
/// receiver or target. Ordinary `obj.method` reads still use
/// [`js_class_method_bind`] and its canonical per-class value identity.
///
/// An own value that already exists wins at read time. This keeps constructor
/// arrow overrides (`this.m = () => ...; const f = this.m`) on the ordinary
/// property path instead of replacing them with a prototype-method snapshot.
#[no_mangle]
pub extern "C" fn js_class_method_snapshot_bind(
    instance: f64,
    method_name_ptr: *const u8,
    method_name_len: usize,
) -> f64 {
    let value = JSValue::from_bits(instance.to_bits());
    if !value.is_pointer()
        || class_registry::is_class_object_value(instance)
        || class_id_from_method_receiver(instance).is_none()
    {
        return js_class_method_bind(instance, method_name_ptr, method_name_len);
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let instance_handle = scope.root_nanbox_f64(instance);
    if !method_name_ptr.is_null() && method_name_len > 0 {
        let key = crate::string::js_string_from_bytes(method_name_ptr, method_name_len as u32);
        let key_handle = scope.root_string_ptr(key);
        let current = instance_handle.get_nanbox_f64();
        let obj = JSValue::from_bits(current.to_bits()).as_pointer::<ObjectHeader>();
        if crate::value::addr_class::is_above_handle_band(obj as usize) {
            let own = key_handle.with_const_ptr::<crate::StringHeader, _>(|key| unsafe {
                super::own_data_field_by_name(obj, key)
            });
            if let Some(own) = own {
                if own.bits() != crate::value::TAG_UNDEFINED {
                    return f64::from_bits(own.bits());
                }
            }
        }
    }

    build_bound_method_closure(
        instance_handle.get_nanbox_f64(),
        method_name_ptr,
        method_name_len,
    )
}

/// By-ID sibling of `js_class_method_bind` for static-name lowering.
///
/// Current codegen passes an immutable AOT descriptor. Legacy heap/short-string
/// ids remain accepted for ABI compatibility.
#[no_mangle]
pub extern "C" fn js_class_method_bind_by_id(instance: f64, method_id: i64) -> f64 {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(name_ref) = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
    else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    js_class_method_bind(instance, name_ref.ptr, name_ref.len)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_CLASS_METHOD_BIND_BY_ID: extern "C" fn(f64, i64) -> f64 = js_class_method_bind_by_id;

#[cfg(test)]
thread_local! {
    static TEST_COLLECT_BOUND_METHOD_AFTER_CAPTURE_INIT: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
    static TEST_BOUND_METHOD_MOVE: std::cell::Cell<(usize, usize)> =
        const { std::cell::Cell::new((0, 0)) };
}

#[cfg(test)]
pub(crate) fn test_collect_bound_method_after_capture_init() {
    TEST_COLLECT_BOUND_METHOD_AFTER_CAPTURE_INIT.with(|armed| armed.set(true));
    TEST_BOUND_METHOD_MOVE.with(|trace| trace.set((0, 0)));
}

#[cfg(test)]
pub(crate) fn test_take_bound_method_move() -> (usize, usize) {
    TEST_BOUND_METHOD_MOVE.with(|trace| trace.replace((0, 0)))
}

pub(super) fn build_bound_method_closure_with_private_brand(
    instance: f64,
    method_name_ptr: *const u8,
    method_name_len: usize,
    private_brand: Option<f64>,
) -> f64 {
    // `js_closure_alloc` can collect before it returns, so keep the receiver
    // live across that allocation. The metadata installation below allocates a
    // string for `.name` and can collect again; keep the newly-created closure
    // in an outer handle and reload it after every such call. Without the outer
    // handle, `set_bound_native_closure_name` protected the closure only inside
    // its own scope and this function could return the now-forwarded from-space
    // address. A caller such as Next's Reflect.get adapter observes that stale
    // method value at an immediately-following `typeof` check (#8036).
    let scope = crate::gc::RuntimeHandleScope::new();
    let instance_handle = scope.root_nanbox_f64(instance);
    let private_brand_handle = private_brand.map(|brand| scope.root_nanbox_f64(brand));
    let closure_handle = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::closure::BOUND_METHOD_FUNC_PTR,
        if private_brand_handle.is_some() { 4 } else { 3 },
    ));
    // Capture-slot writes are scoped arguments to non-allocating stores, so
    // the address cannot go stale inside the call. Each value is read from its
    // own handle first, exactly as before.
    let instance_value = instance_handle.get_nanbox_f64();
    closure_handle.with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure| {
        crate::closure::js_closure_set_capture_f64(closure, 0, instance_value);
        crate::closure::js_closure_set_capture_ptr(closure, 1, method_name_ptr as i64);
        crate::closure::js_closure_set_capture_ptr(closure, 2, method_name_len as i64);
        if let Some(brand) = &private_brand_handle {
            crate::closure::js_closure_set_capture_f64(closure, 3, brand.get_nanbox_f64());
        }
    });
    #[cfg(test)]
    TEST_COLLECT_BOUND_METHOD_AFTER_CAPTURE_INIT.with(|armed| {
        if armed.replace(false) {
            let before = closure_handle
                .with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure| closure as usize);
            // The reload IS the subject of this hook: `across_mut` hands back
            // the post-collection address without ever binding a pre-call one.
            let (_, after) = closure_handle
                .across_mut::<crate::closure::ClosureHeader, _>(crate::gc::gc_collect_minor);
            let after = after as usize;
            TEST_BOUND_METHOD_MOVE.with(|trace| trace.set((before, after)));
        }
    });
    if !method_name_ptr.is_null() && method_name_len > 0 {
        if let Ok(name) = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(method_name_ptr, method_name_len))
        } {
            closure_handle.with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure| {
                set_bound_native_closure_name(closure, name)
            });
            if let Some(length) = bound_native_method_length(name) {
                closure_handle.with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure| {
                    set_builtin_closure_length(closure as usize, length)
                });
            } else if let Some(class_id) =
                class_id_from_method_receiver(instance_handle.get_nanbox_f64())
            {
                // User class method bound as a value (`C.prototype.m`, `c.m`):
                // stamp its spec `.length` from the registered param count so
                // `C.prototype.m.length` reflects the declared arity instead of
                // the closure's capture count (Test262 method `.length` tests).
                if let Some(length) =
                    super::class_registry::class_method_bind_length(class_id, name)
                {
                    closure_handle.with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure| {
                        set_builtin_closure_length(closure as usize, length)
                    });
                }
            }
        }
    }
    closure_handle.with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure| {
        crate::value::js_nanbox_pointer(closure as i64)
    })
}
