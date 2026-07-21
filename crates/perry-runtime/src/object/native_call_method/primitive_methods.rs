use super::super::*;
use super::typed_array::*;
use super::*;

pub(super) unsafe fn dispatch_primitive(
    root_scope: &crate::gc::RuntimeHandleScope,
    object_handle: &crate::gc::RuntimeHandle,
    arg_handles: &[crate::gc::RuntimeHandle],
    object: f64,
    method_name: &str,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let jsval = JSValue::from_bits(object.to_bits());
    let raw_bits = object.to_bits();
    let refreshed_args = || crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(arg_handles);
    let _ = (root_scope, object_handle, &refreshed_args, raw_bits, jsval);
    let _ = (method_name_ptr, method_name_len);
    // Temporal cell (#4686): `duration.add(x)`, `instant.toString()`, etc. A
    // `Temporal.*` value is a NaN-boxed pointer to a custom cell with no
    // codegen fast-path, so every method call funnels through here. The router
    // throws `TypeError` for an unknown method name on a real Temporal receiver.
    #[cfg(feature = "temporal")]
    if crate::temporal::is_temporal_value(object) {
        let args = refreshed_args();
        return Some(crate::temporal::dispatch::call_method(
            object,
            method_name,
            &args,
        ));
    }

    if (object.to_bits() >> 48) == 0x7FFE {
        let class_id = (object.to_bits() & 0xFFFF_FFFF) as u32;
        if crate::object::class_prototype_ref_id(object).is_some() {
            if let Some((func_ptr, param_count, has_synthetic_arguments, has_rest)) =
                crate::object::class_registry::lookup_class_method_in_chain(class_id, method_name)
            {
                return Some(crate::object::class_registry::call_vtable_method(
                    func_ptr,
                    object.to_bits() as i64,
                    args_ptr,
                    args_len,
                    param_count,
                    has_synthetic_arguments,
                    has_rest,
                ));
            }
        } else if class_id != 0
            && crate::object::class_registry::lookup_static_method_in_chain(class_id, method_name)
                .is_some()
        {
            let args = refreshed_args();
            return Some(crate::object::class_registry::js_class_static_method_call(
                object_handle.get_nanbox_f64(),
                method_name_ptr as *const u8,
                method_name_len,
                args.as_ptr(),
                args.len(),
            ));
        } else if class_id != 0 && !method_name_ptr.is_null() && method_name_len > 0 {
            // #5437: `C.viaFn()` where `viaFn` is a static DATA property holding a
            // callable (`C.viaFn = fn` / `static viaFn = fn`), NOT a registered
            // static method. A class reference VALUE is an INT32-tagged class id,
            // not a heap object, so the generic object field-scan below can't deref
            // it; and these statics live in CLASS_DYNAMIC_PROPS, not the static-
            // method vtable, so the arm above misses them. The bug surfaced as a
            // method call on a class returned from / aliased through a function
            // (`const D = C; D.viaFn()`), where the static analyzer couldn't prove
            // the receiver is a class object and lowered it to this dynamic path.
            // Resolve the property exactly as the read-then-call path does
            // (`js_object_get_field_by_name` walks the class-ref static chain),
            // then invoke the callable with `this` bound to the class ref —
            // mirroring `const f = C.viaFn; f()`, which already worked.
            let key_ptr = crate::string::js_string_from_bytes(
                method_name_ptr as *const u8,
                method_name_len as u32,
            );
            let prop =
                js_object_get_field_by_name(object.to_bits() as *const ObjectHeader, key_ptr);
            let prop_bits = prop.bits();
            let raw = (prop_bits & crate::value::POINTER_MASK) as usize;
            if (prop_bits & crate::value::TAG_MASK) == crate::value::POINTER_TAG
                && crate::closure::is_closure_ptr(raw)
            {
                // Rebind the closure's reserved `this` slot to the class ref, as
                // the prototype/field method-dispatch arms above do. A static
                // data property holding an object-literal method (`captures_this`)
                // bakes `this` into a capture slot that `IMPLICIT_THIS` alone
                // can't override; `clone_closure_rebind_this` is a no-op for
                // closures that don't capture `this`, so plain functions and
                // arrows are unaffected.
                let bound = crate::closure::clone_closure_rebind_this(
                    prop_bits,
                    object_handle.get_nanbox_f64(),
                );
                let prop_handle = root_scope.root_nanbox_f64(f64::from_bits(bound));
                let args = refreshed_args();
                let prev_this =
                    IMPLICIT_THIS.with(|c| c.replace(object_handle.get_nanbox_f64().to_bits()));
                let result = crate::closure::js_native_call_value(
                    prop_handle.get_nanbox_f64(),
                    args.as_ptr(),
                    args.len(),
                );
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                return Some(result);
            }
        }
    }

    if method_name == "toString" && jsval.is_pointer() {
        // #4101: `fn.toString()` — reconstruct the function's source from the
        // codegen-registered text (or a synthesized native form), rather than
        // falling through to the generic `"[object Object]"`.
        let raw_addr = crate::value::js_nanbox_get_pointer(object) as usize;
        if crate::value::addr_class::is_above_handle_band(raw_addr)
            && crate::closure::is_closure_ptr(raw_addr)
        {
            if let Some(result) = crate::value::function_to_string_method_result(object) {
                return Some(result);
            }
            let func_ptr = (*(raw_addr as *const crate::closure::ClosureHeader)).func_ptr as usize;
            let s = crate::builtins::function_source_for_func_ptr(func_ptr);
            let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            return Some(f64::from_bits(JSValue::string_ptr(str_ptr).bits()));
        }
        let raw = crate::value::js_nanbox_get_pointer(object) as *const u8;
        if !raw.is_null() && crate::object::is_valid_obj_ptr(raw) {
            unsafe {
                let gc = raw.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc).obj_type == crate::gc::GC_TYPE_ERROR {
                    let s = crate::error::js_error_to_string(raw as *mut crate::error::ErrorHeader);
                    return Some(f64::from_bits(JSValue::string_ptr(s).bits()));
                }
            }
        }
    }

    // Primitive-wrapper prototypes (`Number.prototype`, `Boolean.prototype`,
    // `BigInt.prototype`) carry a brand default value (+0 / false / 0n) for
    // valueOf/toString, matching V8. They are ordinary objects with no
    // [[*Data]] slot, so `boxed_primitive_payload` below misses them; without
    // this a fused `Number.prototype.valueOf()` returned the prototype object
    // itself (test262 Number/prototype/valueOf/S15.7.4.4_*).
    if jsval.is_pointer() && matches!(method_name, "valueOf" | "toString" | "toLocaleString") {
        use crate::object::builtin_prototype_value;
        let ob = object.to_bits();
        if ob == builtin_prototype_value("Number").to_bits() {
            match method_name {
                "valueOf" => return Some(0.0),
                _ => {
                    let radix = if args_len >= 1 && !args_ptr.is_null() {
                        *args_ptr
                    } else {
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    };
                    let s = crate::value::js_jsvalue_to_string_radix(0.0, radix);
                    return Some(f64::from_bits(JSValue::string_ptr(s).bits()));
                }
            }
        }
        if ob == builtin_prototype_value("Boolean").to_bits() {
            match method_name {
                "valueOf" => return Some(f64::from_bits(crate::value::TAG_FALSE)),
                _ => {
                    let s = crate::string::js_string_from_bytes(b"false".as_ptr(), 5);
                    return Some(f64::from_bits(JSValue::string_ptr(s).bits()));
                }
            }
        }
    }

    if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(object) {
        // An own `valueOf`/`toString`/`toLocaleString` data property shadows the
        // intrinsic wrapper method: `var s = new String(); s.valueOf =
        // Number.prototype.valueOf; s.valueOf()` must run the *transferred*
        // method (which brand-checks its receiver and throws a TypeError),
        // not this boxed-primitive fast path that unwraps the [[StringData]]
        // (test262 Number/prototype/valueOf/S15.7.4.4_A2_*, Boolean/prototype/
        // valueOf/S15.6.4.3_A2_*). Fall through to the own-property dispatch in
        // `common_methods::dispatch_common` when such a shadow exists.
        if jsval.is_pointer() && matches!(method_name, "valueOf" | "toString" | "toLocaleString") {
            let own = crate::object::js_object_get_own_field_or_undef(
                object,
                method_name.as_ptr(),
                method_name.len(),
            );
            let own_jsv = JSValue::from_bits(own.to_bits());
            if own_jsv.is_pointer()
                && crate::closure::is_closure_ptr(
                    (own.to_bits() & crate::value::POINTER_MASK) as usize,
                )
            {
                return None;
            }
        }
        match method_name {
            "valueOf" => return Some(payload),
            "toString" | "toLocaleString" => {
                let payload_jsv = JSValue::from_bits(payload.to_bits());
                match crate::builtins::boxed_primitive_to_string_tag(object) {
                    Some("String") => return Some(payload),
                    Some("Number") => {
                        let n = if payload_jsv.is_number() {
                            payload_jsv.as_number()
                        } else {
                            payload
                        };
                        let s = if n.fract() == 0.0
                            && n.abs() < crate::builtins::INT_EXACT_FASTPATH_LIMIT
                        {
                            (n as i64).to_string()
                        } else {
                            n.to_string()
                        };
                        let str_ptr =
                            crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                        return Some(f64::from_bits(JSValue::string_ptr(str_ptr).bits()));
                    }
                    Some("Boolean") => {
                        let s = if payload_jsv.is_bool() && payload_jsv.as_bool() {
                            b"true".as_slice()
                        } else {
                            b"false".as_slice()
                        };
                        let str_ptr =
                            crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                        return Some(f64::from_bits(JSValue::string_ptr(str_ptr).bits()));
                    }
                    Some("BigInt") => {
                        let big = crate::value::JSValue::from_bits(payload.to_bits());
                        if big.is_bigint() {
                            let ptr = crate::bigint::clean_bigint_ptr(
                                (payload.to_bits() & 0x0000_FFFF_FFFF_FFFF)
                                    as *const crate::bigint::BigIntHeader,
                            );
                            let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                            return Some(f64::from_bits(JSValue::string_ptr(str_ptr).bits()));
                        }
                    }
                    Some("Symbol") => {
                        let str_ptr =
                            crate::symbol::js_symbol_to_string(payload) as *mut crate::StringHeader;
                        return Some(f64::from_bits(JSValue::string_ptr(str_ptr).bits()));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if crate::web_storage::is_storage_value(object_handle.get_nanbox_f64()) {
        let args = refreshed_args();
        if let Some(result) = crate::web_storage::dispatch_storage_method(
            object_handle.get_nanbox_f64(),
            method_name,
            &args,
        ) {
            return Some(result);
        }
    }

    // #1758 / epic #1785: a class-object VALUE reaching the *dynamic*
    // dispatcher is a STATIC method call. This happens when the static
    // analyzer couldn't prove the receiver is a class object — e.g.
    // `class X extends (make(...) as any).annotations(y) {}` where the
    // `make()` factory call wasn't inlined to a `ClassExprFresh` (so the
    // `.annotations` receiver lowers to a generic Call result), or any
    // `(expr-returning-a-class-object).staticMethod()`. The compile-time
    // static-dispatch tower (property_get.rs) binds `this` via
    // IMPLICIT_THIS; the generic field-scan path below does NOT, so
    // `this.<staticField>` (effect's `annotations() { make(this.ast, ...) }`)
    // read `undefined`. Route to `js_class_static_method_call`, which binds
    // `this` to the receiver and walks the class_id parent chain — but only
    // when the method actually resolves in the static chain, so an own
    // function-valued static field still falls through to the generic path.
    if crate::object::class_registry::is_class_object_value(object) {
        let class_id = crate::object::js_object_get_class_id(jsval.as_pointer::<ObjectHeader>());
        if class_id != 0
            && crate::object::class_registry::lookup_static_method_in_chain(class_id, method_name)
                .is_some()
        {
            let args = refreshed_args();
            return Some(crate::object::class_registry::js_class_static_method_call(
                object_handle.get_nanbox_f64(),
                method_name_ptr as *const u8,
                method_name_len,
                args.as_ptr(),
                args.len(),
            ));
        }
    }

    // #5142: a promise can carry user-attached own expando methods.
    // @tanstack/query-core's `pendingThenable()` stores `resolve`/`reject`
    // closures on the thenable and invokes them as `thenable.resolve(value)`;
    // an own expando function shadows the inherited prototype method, so
    // resolve and call it here before the intrinsic then/catch/finally and the
    // generic "<m> is not a function" fall-through. Only dispatch when the
    // stored value is actually callable — a non-callable expando
    // (`thenable.status()`) falls through to the normal not-a-function path.
    //
    // #5590: this INCLUDES `then`/`catch`/`finally`. A user-assigned own
    // `then` (`p.then = function(){…}`) must shadow the intrinsic — the spec's
    // `Invoke(promise, "then", …)` does a `Get` of the property, which finds
    // the own override. The Promise combinators rely on this: `Promise.all`'s
    // per-element `Invoke(nextPromise, "then", «resolveElement, reject»)` must
    // dispatch to a custom `then` when present (test262 all/race/allSettled/any
    // `invoke-then.js`). A callable own override wins here; a promise without
    // one falls through to the intrinsic then/catch/finally block below.
    if crate::promise::js_value_is_promise(object_handle.get_nanbox_f64()) != 0 {
        let recv = object_handle.get_nanbox_f64();
        let raw = (recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
        if let Some(v) = super::exotic_expando::exotic_get_own_property(
            raw,
            super::exotic_expando::ExoticKind::Promise,
            method_name,
            recv,
        ) {
            let cand = (v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
            if (v.to_bits() & crate::value::TAG_MASK) == crate::value::POINTER_TAG
                && crate::closure::is_closure_ptr(cand)
            {
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(recv.to_bits()));
                let result = crate::closure::js_native_call_value(v, args_ptr, args_len);
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                return Some(result);
            }
        }
    }

    // Issue #489 followup: Promise's `then` / `catch` / `finally` are
    // intrinsic — when the dynamic dispatch path lands a `.then(cb)` on
    // a Promise (drizzle's `mysql-proxy/session.js`:
    // `this.client(...).then(({rows}) => rows)` where the static
    // analyzer couldn't prove the receiver is a Promise), route directly
    // to `js_promise_then` / `js_promise_catch` / `js_promise_finally`.
    // Without this, the field-scan + class-id walks below find nothing
    // and return undefined — drizzle's `MySqlRemoteSession.all` then
    // resolves to undefined and downstream `data[0].insertId` accesses
    // silently fail.
    if matches!(method_name, "then" | "catch" | "finally")
        && crate::promise::js_value_is_promise(object_handle.get_nanbox_f64()) != 0
    {
        let promise_val = object_handle.get_nanbox_f64();
        let promise_ptr = (promise_val.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut crate::Promise;
        let promise_handle = root_scope.root_raw_mut_ptr(promise_ptr);

        // Check for own properties that require the spec path:
        //  - own "then": the user replaced the method (spy / non-callable / accessor)
        //  - own "constructor": SpeciesConstructor must read it (then/catch/finally all
        //    chain a new promise through the species constructor)
        let promise_addr = promise_handle.get_raw_mut_ptr::<crate::Promise>() as usize;
        let has_own_then = crate::promise::promise_has_own_property(promise_addr, "then");
        let has_own_ctor = crate::promise::promise_has_own_constructor(promise_addr);

        if has_own_then || has_own_ctor {
            // Spec path: look up the prototype method and call it with
            // IMPLICIT_THIS set to the promise value so the thunk can read it.
            if has_own_then && method_name == "then" {
                // For "then" with own "then": Invoke(promise, "then", args) directly.
                // exotic_get_own_property invokes accessor getters (propagating throws)
                // and returns the data value; is_callable_value checks callability.
                let own_then = unsafe {
                    crate::object::exotic_expando::exotic_get_own_property(
                        promise_addr,
                        crate::object::exotic_expando::ExoticKind::Promise,
                        "then",
                        promise_val,
                    )
                }
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                if !crate::promise::spec_combinators::is_callable_value(own_then) {
                    let msg = b"'then' property on Promise is not callable";
                    let msg_str =
                        crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
                    let err = crate::error::js_typeerror_new(msg_str);
                    crate::exception::js_throw(f64::from_bits(
                        JSValue::pointer(err as *const u8).bits(),
                    ));
                }
                let args = refreshed_args();
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(promise_val.to_bits()));
                let result = unsafe {
                    crate::closure::js_native_call_value(own_then, args.as_ptr(), args.len())
                };
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                return Some(result);
            }
            // For "catch"/"finally" with own "then", or "then" with own "constructor":
            // call the prototype thunk with IMPLICIT_THIS = promise_val so it can
            // read the receiver and invoke call_receiver_then / SpeciesConstructor.
            if let Some(proto_method) = crate::promise::promise_proto_method(method_name) {
                let args = refreshed_args();
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(promise_val.to_bits()));
                let result = unsafe {
                    crate::closure::js_native_call_value(proto_method, args.as_ptr(), args.len())
                };
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                return Some(result);
            }
            // Fallthrough to fast path if prototype method lookup fails.
        }

        let args = refreshed_args();
        let arg0_box = if !args.is_empty() {
            args[0]
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        let arg1_box = if args.len() >= 2 {
            args[1]
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        // Closures arrive here in two shapes:
        //  - NaN-boxed `POINTER_TAG | (closure_ptr & 0x0000_FFFF_FFFF_FFFF)`
        //    (the codegen `js_closure_alloc_singleton` + OR-with-tag form)
        //  - Raw `*ClosureHeader` bit-cast to f64 — the convention used
        //    by `js_assimilate_thenable` when it propagates
        //    `then(resolve, reject)` callbacks through a user-defined
        //    `then` method's param slots (see `promise.rs:2438-2442`).
        // Accept both. TAG_UNDEFINED / null / non-pointer values stay
        // null so `js_promise_then` treats the handler as missing.
        let extract_closure = |v: f64| -> crate::promise::ClosurePtr {
            let b = v.to_bits();
            let candidate = if (b & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
                b & 0x0000_FFFF_FFFF_FFFF
            } else if (b & 0xFFFF_0000_0000_0000) == 0 {
                b
            } else {
                0
            };
            if candidate < 0x10000 {
                std::ptr::null()
            } else {
                candidate as crate::promise::ClosurePtr
            }
        };
        let result = match method_name {
            "then" => crate::promise::js_promise_then(
                promise_handle.get_raw_mut_ptr(),
                extract_closure(arg0_box),
                extract_closure(arg1_box),
            ),
            "catch" => crate::promise::js_promise_catch(
                promise_handle.get_raw_mut_ptr(),
                extract_closure(arg0_box),
            ),
            "finally" => crate::promise::js_promise_finally(
                promise_handle.get_raw_mut_ptr(),
                extract_closure(arg0_box),
            ),
            _ => unreachable!(),
        };
        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
    }

    // `regex.test(str)` / `regex.exec(str)` on an *untyped* receiver — e.g.
    // hono's RegExpRouter does `buildWildcardRegExp(k).test(path)`, a call on a
    // function result the codegen `Expr::RegExpTest` fast path can't see; without
    // this it throws `test is not a function`, breaking Hono `app.use('*', …)`
    // (#1731). The helper returns None for non-regex so generic dispatch resumes.
    #[cfg(feature = "regex-engine")]
    if matches!(method_name, "test" | "exec" | "toString") && jsval.is_pointer() {
        let p = jsval.as_pointer::<u8>();
        // An OWN property SHADOWS the `RegExp.prototype` method — ordinary
        // `[[Get]]` consults the receiver's own properties before walking the
        // prototype chain. `__re.toString = Object.prototype.toString;
        // __re.toString()` must therefore call the *assigned* function (giving
        // `[object RegExp]`), not the builtin `RegExp.prototype.toString`
        // (which returns the `/source/flags` literal). The same holds for
        // `re.exec` / `re.test` overrides, which libraries use to instrument a
        // regex. Expando writes on a RegExp land in the `exotic_expando` side
        // table, because a `RegExpHeader` is not an `ObjectHeader`.
        //
        // The own value is invoked HERE rather than by declining the regex
        // dispatch and letting the generic path pick it up: `toString` has a
        // downstream catch-all arm that stringifies any pointer receiver via
        // `js_jsvalue_to_string`, which maps a regex straight back to
        // `/source/flags` — so a bare fall-through would still miss the
        // override (test262 built-ins/RegExp/S15.10.4.1_A6_T1, #5897).
        // `exotic_get_own_property` rather than `value_lookup`: the override
        // may be an ACCESSOR (`Object.defineProperty(re, "test", { get() {…} })`),
        // which `value_lookup` — a data-property-only side-table read — cannot
        // see, so the builtin would run instead. It checks accessor descriptors
        // first (invoking the getter with `object` as the receiver) and falls
        // back to the same expando data lookup.
        let own_override = if crate::regex::is_regex_pointer(p) {
            crate::object::exotic_expando::exotic_get_own_property(
                p as usize,
                crate::object::exotic_expando::ExoticKind::RegExp,
                method_name,
                object,
            )
            .map(|v| v.to_bits())
        } else {
            None
        };
        match own_override {
            Some(own_bits) => {
                let raw = (own_bits & crate::value::POINTER_MASK) as usize;
                if (own_bits & crate::value::TAG_MASK) == crate::value::POINTER_TAG
                    && crate::closure::is_closure_ptr(raw)
                {
                    // Bind `this` to the regex, exactly as the class-static and
                    // prototype method-dispatch arms above do.
                    let bound = crate::closure::clone_closure_rebind_this(
                        own_bits,
                        object_handle.get_nanbox_f64(),
                    );
                    let prop_handle = root_scope.root_nanbox_f64(f64::from_bits(bound));
                    let args = refreshed_args();
                    let prev_this =
                        IMPLICIT_THIS.with(|c| c.replace(object_handle.get_nanbox_f64().to_bits()));
                    let result = crate::closure::js_native_call_value(
                        prop_handle.get_nanbox_f64(),
                        args.as_ptr(),
                        args.len(),
                    );
                    IMPLICIT_THIS.with(|c| c.set(prev_this));
                    return Some(result);
                }
                // Own key present but NOT callable (`re.exec = 5; re.exec()`).
                // Fall through to generic dispatch, which raises the
                // `is not a function` TypeError — never run the builtin.
            }
            None => {
                let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
                let arg0 = refreshed_args().first().copied().unwrap_or(undef);
                if let Some(r) = crate::regex::dispatch_regex_receiver_method(p, method_name, arg0)
                {
                    return Some(r);
                }
            }
        }
    }

    // `RegExp.prototype.compile(pattern, flags)` (Annex B) re-initializes the
    // receiver in place. Needs both args, so it is dispatched here rather than
    // through the single-arg `dispatch_regex_receiver_method`.
    #[cfg(feature = "regex-engine")]
    if method_name == "compile" && jsval.is_pointer() {
        let p = jsval.as_pointer::<u8>();
        if crate::regex::is_regex_pointer(p) {
            let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
            let args = refreshed_args();
            let pat = args.first().copied().unwrap_or(undef);
            let flags = args.get(1).copied().unwrap_or(undef);
            return Some(crate::regex::js_regexp_compile_value(
                p as *mut crate::regex::RegExpHeader,
                pat,
                flags,
            ));
        }
    }

    // Node timer handles are represented in Perry as small integer ids
    // NaN-boxed as pointers. Provide the common Timeout/Immediate methods
    // directly so `timeout.ref().unref().hasRef()` style probes behave like
    // Node without having to allocate a full JS wrapper object per timer.
    //
    // Gated on (a) tag == POINTER_TAG (0x7FFD) to avoid catching strings /
    // int32 / nullish tags, and (b) the id being a known timer so unrelated
    // small handles (UI widgets, drizzle, native instances) fall through
    // to the normal dispatch.
    {
        let bits = object.to_bits();
        let top16 = bits >> 48;
        if top16 == 0x7FFD {
            let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
            // Timer ids and `perry-ffi` registry handles share the pointer-tagged
            // small-integer band and both count from 1, so a bare id can be
            // ambiguous (e.g. an HTTP/2 server handle 1 vs a `setTimeout` id 1
            // alive at the same time). A live registered handle is the
            // authoritative interpretation — it owns a real Rust object and its
            // method surface (`close`/`ref`/`unref`/…) — so yield to the handle
            // dispatch below rather than swallow `server.close()` as
            // `clearTimeout`. A genuine timer whose id does not also name a live
            // handle still resolves here.
            if crate::timer::is_known_timer_id(id) && !super::class_handles::ffi_handle_exists(id) {
                match method_name {
                    "ref" => {
                        crate::timer::js_timer_ref(id);
                        return Some(object);
                    }
                    "unref" => {
                        crate::timer::js_timer_unref(id);
                        return Some(object);
                    }
                    "hasRef" => {
                        return Some(if crate::timer::js_timer_has_ref(id) != 0 {
                            f64::from_bits(JSValue::bool(true).bits())
                        } else {
                            f64::from_bits(JSValue::bool(false).bits())
                        });
                    }
                    "refresh" => {
                        crate::timer::js_timer_refresh(id);
                        return Some(object);
                    }
                    "close" => {
                        crate::timer::clearTimeout(id);
                        crate::timer::clearInterval(id);
                        crate::timer::clearImmediate(id);
                        return Some(object);
                    }
                    // `__perry_dispose__` is the class-member form; the
                    // well-known `Symbol.dispose` computed form lowers to
                    // `@@__perry_wk_dispose`. Both clear the timer (#1213).
                    "__perry_dispose__" | "@@__perry_wk_dispose" => {
                        crate::timer::clearTimeout(id);
                        crate::timer::clearInterval(id);
                        crate::timer::clearImmediate(id);
                        return Some(f64::from_bits(JSValue::undefined().bits()));
                    }
                    "@@__perry_wk_toPrimitive" | "valueOf" => return Some(id as f64),
                    _ => {}
                }
            }
        }
    }

    // A `DateCell` is a NaN-boxed pointer but NOT an `ObjectHeader`, so a date
    // receiver must never reach the generic object dispatch below — that path
    // reinterprets the cell's bytes as an object and returns garbage. Every
    // `Date.prototype` method (getters, setters, `toISOString`, `toJSON`,
    // `toString`, …) is installed on `Date.prototype` and reads the
    // `IMPLICIT_THIS` receiver, so resolve the method there and dispatch with
    // `this` bound to the cell. Previously only `toString` was routed this way;
    // every other dynamic/computed call (`date[m](...)`, `Reflect.apply`) fell
    // through and silently dropped setter mutations — e.g. dayjs's
    // `this.$d[l]($)` made `.add()`/`.date(n)` no-ops (#5133).
    if crate::date::is_date_value(object) {
        // An own callable expando shadows the intrinsic Date.prototype method:
        // `Object.defineProperty(d, "toString", {value: Number.prototype.toString});
        // d.toString()` must run the *transferred* method (which brand-checks its
        // receiver and throws a TypeError), not Date.prototype.toString (test262
        // Number/prototype/{toString,valueOf}/*_A*_T03 and Boolean/prototype/
        // {toString,valueOf}/*_A2_T3 — transfer-to-Date). Date instances are
        // DateCell exotics, so own props live in the exotic expando table, not
        // the ordinary object keys_array read by js_object_get_own_field_or_undef.
        let recv_bits = object.to_bits();
        let recv_addr = (recv_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if let Some(v) = super::exotic_expando::exotic_get_own_property(
            recv_addr,
            super::exotic_expando::ExoticKind::Date,
            method_name,
            object,
        ) {
            if (v.to_bits() & crate::value::TAG_MASK) == crate::value::POINTER_TAG
                && crate::closure::is_closure_ptr((v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize)
            {
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(recv_bits));
                let result = crate::closure::js_native_call_value(v, args_ptr, args_len);
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                return Some(result);
            }
        }
        let ctor = crate::object::js_get_global_this_builtin_value(b"Date".as_ptr(), 4);
        let ctor_ptr = crate::value::js_nanbox_get_pointer(ctor) as usize;
        if ctor_ptr != 0 {
            let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
            if let Some(proto_ptr) = object_ptr_from_value(proto) {
                let key = crate::string::js_string_from_bytes(
                    method_name_ptr as *const u8,
                    method_name_len as u32,
                );
                let value = crate::object::js_object_get_field_by_name(proto_ptr, key);
                if !value.is_undefined() {
                    let value_f64 = f64::from_bits(value.bits());
                    let prev_this = IMPLICIT_THIS.with(|c| c.replace(object.to_bits()));
                    let result =
                        crate::closure::js_native_call_value(value_f64, args_ptr, args_len);
                    IMPLICIT_THIS.with(|c| c.set(prev_this));
                    return Some(result);
                }
            }
        }
        if method_name == "toString" {
            let string = crate::date::js_date_to_string(object);
            return Some(f64::from_bits(JSValue::string_ptr(string).bits()));
        }
    }

    // Symbols: Symbol.for() pointers are Box-leaked (no GcHeader), so the
    // ObjectHeader path below would dereference garbage. Detect symbols
    // up front via the side-table.
    if jsval.is_pointer() {
        let raw_ptr = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
        if crate::symbol::is_registered_symbol(raw_ptr) {
            let sym_f64 = object;
            return Some(match method_name {
                "toString" => {
                    let s = crate::symbol::js_symbol_to_string(sym_f64);
                    f64::from_bits(JSValue::string_ptr(s as *mut crate::StringHeader).bits())
                }
                "valueOf" => sym_f64,
                "description" => {
                    f64::from_bits(crate::symbol::js_symbol_description(sym_f64).to_bits())
                }
                _ => f64::from_bits(crate::value::TAG_UNDEFINED),
            });
        }
    }

    // Handle BigInt method calls (NaN-boxed with BIGINT_TAG 0x7FFA)
    if jsval.is_bigint() {
        let bigint_ptr = crate::bigint::clean_bigint_ptr(
            (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader,
        );
        match method_name {
            "isZero" => {
                let result = crate::bigint::js_bigint_is_zero(bigint_ptr);
                return Some(f64::from_bits(JSValue::bool(result != 0).bits()));
            }
            "isNeg" | "isNegative" => {
                let result = crate::bigint::js_bigint_is_negative(bigint_ptr);
                return Some(f64::from_bits(JSValue::bool(result != 0).bits()));
            }
            "toNumber" => {
                return Some(crate::bigint::js_bigint_to_f64(bigint_ptr));
            }
            "toString" => {
                // #2864: ToNumber/ToInteger-coerce + validate the radix
                // (RangeError for out-of-range), `None`/no-arg → decimal.
                let radix = if args_len > 0 && !args_ptr.is_null() {
                    crate::value::coerce_validate_radix(*args_ptr)
                } else {
                    None
                };
                let result_ptr = match radix {
                    Some(r) => crate::bigint::js_bigint_to_string_radix(bigint_ptr, r),
                    None => crate::bigint::js_bigint_to_string(bigint_ptr),
                };
                return Some(f64::from_bits(JSValue::string_ptr(result_ptr).bits()));
            }
            "add" | "sub" | "mul" | "div" | "mod" | "umod" | "pow" | "and" | "or" | "xor"
            | "shln" | "shrn" | "maskn" | "eq" | "lt" | "lte" | "gt" | "gte" | "cmp"
            | "fromTwos" | "toTwos" => {
                let args = refreshed_args();
                return Some(dispatch_bigint_binary_method(
                    bigint_ptr,
                    method_name,
                    args.as_ptr(),
                    args.len(),
                ));
            }
            _ => {
                // Unknown BigInt method - fall through to general dispatch
            }
        }
    }

    // Check for raw handle integer: Perry may bit-cast an i64 handle directly to f64,
    // producing a subnormal float (bits == handle_id, no NaN-box tag). Untagged values
    // in the handle band are raw handle IDs from Perry's integer-typed handle parameters.
    let raw_bits = object.to_bits();
    if crate::value::addr_class::is_small_handle(raw_bits as usize) {
        if let Some(dispatch) = handle_method_dispatch() {
            let args = refreshed_args();
            return Some(dispatch(
                raw_bits as i64,
                method_name.as_ptr(),
                method_name.len(),
                args.as_ptr(),
                args.len(),
            ));
        }
        // No handle dispatcher registered: return JS `undefined`, NOT the
        // signaling-NaN bit pattern 0x7FF8_..._0001 (a JS *number*) that a prior
        // copy of this line used. See the JS-handle fallback above for why the
        // sNaN surfaced as a spurious "Iterator result is not an object".
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }

    // #1545: Web Streams handles are returned as `id as f64` (a normal float),
    // so their `to_bits()` is large and the raw-handle check above misses them.
    // When the receiver is a finite whole number and the stdlib probe confirms
    // it's a live stream handle, route the call through the same handle
    // dispatcher (which carries the stream method arms). Gating on the probe
    // means a genuine numeric receiver calling an unknown method still falls
    // through to the `(number).x is not a function` TypeError below.
    if object.is_finite() && object > 0.0 && object.fract() == 0.0 {
        let id = object as usize;
        if let Some(probe) = stream_handle_probe() {
            if probe(id) {
                if let Some(dispatch) = handle_method_dispatch() {
                    let args = refreshed_args();
                    return Some(dispatch(
                        id as i64,
                        method_name.as_ptr(),
                        method_name.len(),
                        args.as_ptr(),
                        args.len(),
                    ));
                }
            }
        }
    }

    // Issue #654: typed-array method dispatch. The codegen for
    // `new Float64Array(...)` (and the other typed-array constructors)
    // returns the raw heap pointer bitcast to f64 — no POINTER_TAG —
    // so neither `is_pointer()` nor the handle dispatch above catches
    // it. Detect via the `TYPED_ARRAY_REGISTRY` side table and route
    // common methods (`sort`, `at`, `toSorted`, `toReversed`, `with`,
    // `findLast`, `findLastIndex`) to their `js_typed_array_*` runtime
    // helpers. Without this arm `(a: Float64Array).sort()` reached the
    // `(number).sort is not a function` catch-all because raw pointer
    // bits classify as `is_number()` (top16 outside the tagged range).
    {
        let top16 = raw_bits >> 48;
        if top16 == 0 && raw_bits >= 0x10000 {
            let addr = raw_bits as usize;
            if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
                let ta = addr as *mut crate::typedarray::TypedArrayHeader;
                if let Some(r) = dispatch_typed_array_method(ta, method_name, args_ptr, args_len) {
                    return Some(r);
                }
            }
        }
    }

    None
}
