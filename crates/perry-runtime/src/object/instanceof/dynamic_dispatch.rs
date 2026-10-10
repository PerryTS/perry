//! `js_instanceof_dynamic` — the dynamic (runtime-class-ref) form of
//! `instanceof`, resolving a value/class-ref RHS pair rather than a
//! compile-time-known class id.
//!
//! Split out of `instanceof.rs` for the file-size cap. Pure relocation —
//! no logic changes; see `super::*` for every helper this calls.

use super::*;

#[no_mangle]
pub extern "C" fn js_instanceof_dynamic(value: f64, mut type_ref: f64) -> f64 {
    // Proxy ids are registry handles, not closure headers. Resolve their
    // observable @@hasInstance/prototype reads before any constructor probe
    // or unwrapping of the left operand (#10364).
    if crate::proxy::js_proxy_is_proxy(type_ref) != 0 {
        return super::proxy_rhs::proxy_instanceof(value, type_ref);
    }
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    // `proxy instanceof C` uses the proxy's `[[GetPrototypeOf]]`, which (absent a
    // trap) forwards to the target — so it is equivalent to `target instanceof
    // C`. The proxy itself is a small registered id with no class chain, so
    // without this it always returned false. Unwrap nested proxies (drizzle
    // aliases columns as `new Proxy(column, …)` and its `is(value, type)` brand
    // check relies on `value instanceof type`). Bounded to guard a cycle.
    let mut value = value;
    {
        let mut depth = 0;
        while depth < 16 && crate::proxy::js_proxy_is_proxy(value) != 0 {
            value = crate::proxy::js_proxy_target(value);
            depth += 1;
        }
    }
    // `temporalValue instanceof Temporal.<X>` — Temporal values dispatch via
    // brand arms (not a real prototype chain), so resolve the constructor to
    // its kind and compare against the value's brand. A non-Temporal value, or
    // a Temporal value of a different kind, yields `false`.
    if let Some(kind) = super::global_this::temporal_ctor_kind(type_ref) {
        if crate::temporal::temporal_kind(value) == Some(kind) {
            return f64::from_bits(crate::value::TAG_TRUE);
        }
        // `class X extends Temporal.<Type>` instance: a plain heap object whose
        // [[Prototype]] chain reaches `Temporal.<Type>.prototype`. It carries
        // the brand via a stashed cell rather than the Temporal-cell tag, so
        // recover that cell and compare its kind. The receiver reaches here both
        // NaN-boxed (top16 == 0x7FFD) and as a raw-I64 heap pointer (top16 == 0,
        // how module-level object vars are stored) — accept both. (#5587)
        {
            let bits = value.to_bits();
            let top16 = bits >> 48;
            let raw = if top16 == 0x7FFD {
                (bits & crate::value::POINTER_MASK) as usize
            } else if top16 == 0 {
                bits as usize
            } else {
                0
            };
            if raw != 0 {
                if let Some(cell) = unsafe { crate::temporal::hooked::subclass_cell(raw) } {
                    if crate::temporal::temporal_kind(cell) == Some(kind) {
                        return f64::from_bits(crate::value::TAG_TRUE);
                    }
                }
            }
        }
        return f64::from_bits(TAG_FALSE);
    }
    // Spec step (InstanceofOperator): an OWN user-defined `@@hasInstance`
    // overrides even native constructor brand checks. The native generic hook
    // lives on Function.prototype, so the own-property gate distinguishes an
    // explicit override from that inherited default without recursion.
    {
        let hi_sym = crate::symbol::well_known_symbol("hasInstance");
        if !hi_sym.is_null() {
            let hi_f64 = f64::from_bits(crate::value::JSValue::pointer(hi_sym as *const u8).bits());
            if unsafe { crate::symbol::js_object_has_own_symbol(type_ref, hi_f64) } {
                // An own @@hasInstance getter can collect even when it returns
                // undefined and ordinary dispatch continues. Refresh both
                // operands before the callback or any later prototype check.
                let scope = crate::gc::RuntimeHandleScope::new();
                let lhs_h = scope.root_nanbox_f64(value);
                let rhs_h = scope.root_nanbox_f64(type_ref);
                let cb = unsafe {
                    crate::symbol::js_object_get_symbol_property(rhs_h.get_nanbox_f64(), hi_f64)
                };
                value = lhs_h.get_nanbox_f64();
                type_ref = rhs_h.get_nanbox_f64();
                if let HasInstanceOutcome::Result(result) = dispatch_own_has_instance(cb, value) {
                    return result;
                }
            }
        }
    }
    let bits = type_ref.to_bits();
    // `class_ref_id` requires `is_class_id_registered`, not just the tag —
    // a user-crafted NaN payload sharing the 0x7FFE band (a real JS number
    // constructed via `DataView.setFloat64`, not a codegen-emitted class
    // ref) must fall through to the unresolved-RHS `TypeError` below
    // instead of being dispatched into `js_instanceof` as a bogus class id.
    if let Some(class_id) = class_ref_id(type_ref) {
        // #11759 (c′): the class function object is the first evaluation of a
        // declaration whose later evaluations share its template id. A value
        // with an individually recorded prototype chain (an instance of a
        // later evaluation) answers by its actual chain.
        if crate::object::class_value::class_value_is_first_evaluation(class_id)
            && super::prototype_chain::object_static_prototype(value_addr(value)).is_some()
        {
            return f64::from_bits(if ordinary_has_instance_prototype_walk(value, type_ref) {
                crate::value::TAG_TRUE
            } else {
                TAG_FALSE
            });
        }
        return js_instanceof(value, class_id);
    }
    // #9502: a heap class object's template id identifies its code, not its
    // evaluation. Compare the actual prototype objects so sibling evaluations
    // remain distinct and a chain through earlier evaluations still matches.
    if is_class_object_value(type_ref) {
        // Static/forward `new C()` sites can still construct by template id
        // without attaching an evaluated prototype. Retain that representation's
        // class-id check; a recorded prototype (a fact of the receiver's
        // shape) is authoritative.
        let obj = crate::JSValue::from_bits(bits).as_pointer::<ObjectHeader>();
        let template = js_object_get_class_id(obj);
        // #11759 (c′): when the template's class function object is the
        // declaration's first evaluation, a template-id instance belongs to
        // THAT evaluation, never to this later one: the chain decides.
        if super::prototype_chain::object_static_prototype(value_addr(value)).is_none()
            && !crate::object::class_value::class_value_is_first_evaluation(template)
        {
            return js_instanceof(value, template);
        }
        return f64::from_bits(if ordinary_has_instance_prototype_walk(value, type_ref) {
            crate::value::TAG_TRUE
        } else {
            TAG_FALSE
        });
    }
    // A primitive is an ordinary miss; a terminal ordinary receiver has a
    // complete prototype chain. Test those facts before classifying the
    // constructor: its actual prototype decides the terminal receiver's
    // OrdinaryHasInstance without any native-body or native-brand dispatch.
    // Bound functions still delegate to their target, and non-callable RHS
    // values retain the general path's TypeError. Own hooks ran above.
    let primitive = instanceof_lhs_is_primitive(value);
    if primitive || terminal_prototype_id(value).is_some() {
        let rhs = crate::JSValue::from_bits(type_ref.to_bits());
        if rhs.is_pointer() {
            let closure = rhs.as_pointer::<crate::closure::ClosureHeader>();
            // One validated info proves callability and exposes the existing
            // bound-body sentinel; do not validate the same callee twice.
            if let Some(info) = crate::closure::closure_info(closure) {
                if info.code == crate::closure::BOUND_FUNCTION_FUNC_PTR {
                    let target = crate::closure::js_closure_get_capture_f64(closure, 0);
                    return js_instanceof_dynamic(value, target);
                }
                if primitive {
                    return f64::from_bits(TAG_FALSE);
                }
                // The existing function shape can also prove an own data
                // prototype. Reading its live slot is Leaf, so both shape
                // proofs can complete without opening any handle scopes.
                if let Some(Some(prototype)) =
                    unsafe { crate::closure::shape::closure_own_prototype_by_shape(closure) }
                {
                    if let Some(matches) = terminal_prototype_matches(value, prototype) {
                        return f64::from_bits(if matches {
                            crate::value::TAG_TRUE
                        } else {
                            TAG_FALSE
                        });
                    }
                }
                return f64::from_bits(if ordinary_has_instance_prototype_walk(value, type_ref) {
                    crate::value::TAG_TRUE
                } else {
                    TAG_FALSE
                });
            }
        }
    }
    // Class values take their existing exits above. An intrinsic's body
    // identity also proves it is callable and not a bind wrapper, so dispatch
    // it before the remaining callable/bound/native-export probes. Its own
    // @@hasInstance already had first refusal; the shared native dispatch owns
    // the primitive and prototype checks.
    if let Some(name) = identify_global_builtin_constructor(type_ref) {
        match name {
            "Crypto" => {
                return if is_native_module_namespace_value(value, "crypto.webcrypto") {
                    f64::from_bits(crate::value::TAG_TRUE)
                } else {
                    f64::from_bits(TAG_FALSE)
                };
            }
            "SubtleCrypto" => {
                return if is_native_module_namespace_value(value, "crypto.subtle") {
                    f64::from_bits(crate::value::TAG_TRUE)
                } else {
                    f64::from_bits(TAG_FALSE)
                };
            }
            "CryptoKey" => {
                let addr = value_addr(value);
                return if addr != 0 && crate::buffer::crypto_key_meta(addr).is_some() {
                    f64::from_bits(crate::value::TAG_TRUE)
                } else {
                    f64::from_bits(TAG_FALSE)
                };
            }
            _ => {}
        }
        let class_id = global_builtin_constructor_class_id(name);
        if class_id != 0 {
            // Own hooks and proxy unwrapping have already completed. Once this
            // actual constructor is proved callable, primitives are a miss;
            // native dispatch need not repeat the static class-hook prefix.
            if instanceof_lhs_is_primitive(value) {
                return f64::from_bits(TAG_FALSE);
            }
            return super::static_dispatch::native_with_constructor(
                value,
                class_id,
                Some(type_ref),
            );
        }
    }
    // Intrinsics and class values have completed their own dispatch. Only
    // remaining function values need the compiled-function ancestry probe;
    // ordinary native constructor checks no longer pay its failed validation.
    // #10507: `x instanceof F` for an ordinary compiled function `F`: one
    // shape compare when `x`'s ShapeId names `F.prototype`, else, for an
    // object of no compiled class, OrdinaryHasInstance's prototype walk.
    {
        use crate::object::class_registry::OrdinaryInstanceof;
        match crate::object::class_registry::ordinary_compiled_function_has_instance(
            value, type_ref,
        ) {
            Some(OrdinaryInstanceof::Instance) => return f64::from_bits(crate::value::TAG_TRUE),
            Some(OrdinaryInstanceof::PrototypeWalk) => {
                return f64::from_bits(if ordinary_has_instance_prototype_walk(value, type_ref) {
                    crate::value::TAG_TRUE
                } else {
                    crate::value::TAG_FALSE
                });
            }
            None => {}
        }
    }
    // OrdinaryHasInstance step 2: a bound function (with no own
    // `@@hasInstance`, answered above) is `instanceof` exactly as its target.
    if let Some(target) = crate::object::class_registry::bound_function_target_value(type_ref) {
        return js_instanceof_dynamic(value, target);
    }
    // Class and intrinsic RHS values have already taken their own exits.
    // A remaining callable still answers false for a primitive; a non-callable
    // RHS must reach TypeError even when the left operand is primitive.
    if instanceof_lhs_is_primitive(value) && value_is_callable(type_ref) {
        return f64::from_bits(TAG_FALSE);
    }
    // Native http(s).Agent handles have no heap prototype chain. After any own
    // override above has had first refusal, retain their native brand check.
    if let Some((module, method)) = unsafe { bound_native_callable_module_and_method(type_ref) } {
        if matches!(module.as_str(), "http" | "https") && method == "Agent" {
            let matched = small_native_handle_id(value)
                .zip(crate::object::http_agent_handle_probe())
                .is_some_and(|(handle, probe)| unsafe { probe(handle) });
            if matched || ordinary_has_instance_prototype_walk(value, type_ref) {
                return f64::from_bits(crate::value::TAG_TRUE);
            }
            return f64::from_bits(TAG_FALSE);
        }
    }
    // #6558: `e instanceof WebAssembly.CompileError` (and LinkError /
    // RuntimeError). These constructors live on the WebAssembly NAMESPACE —
    // not on `globalThis`, so the builtin-name path above never resolves
    // them — and their instances are ErrorHeader-backed values with no
    // prototype chain reaching the namespace ctor's `.prototype`, so the
    // ordinary prototype walk below can't brand them either. Identify the
    // ctor by its dedicated thunk func_ptr (GC-move-safe) and brand-check
    // the instance by its error `.name`.
    if let Some(matches) = super::global_this::webassembly_error_ctor_instanceof(value, type_ref) {
        return f64::from_bits(if matches {
            crate::value::TAG_TRUE
        } else {
            crate::value::TAG_FALSE
        });
    }
    // #6558 sibling: `mod instanceof WebAssembly.Module` for the wasm-host
    // module wrapper. Its `[[Prototype]]` does not reach the namespace ctor's
    // `.prototype`, so brand-check its GC-aware internal wrapper identity.
    // Only a positive match short-circuits here; a miss returns `None` so the
    // value still flows to the prototype walk below (how `WebAssembly.Memory`
    // instances resolve, and how a foreign object answers `false`).
    if let Some(true) = super::global_this::webassembly_value_ctor_instanceof(value, type_ref) {
        return f64::from_bits(crate::value::TAG_TRUE);
    }
    if let Some((module, method)) = unsafe { bound_native_callable_module_and_method(type_ref) } {
        if module == "stream"
            && matches!(
                method.as_str(),
                "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough" | "Stream"
            )
            && (crate::node_stream::is_classic_stream_instance_of(value, method.as_str())
                || super::tls_constructor_prototype_is_instance_of(value, method.as_str())
                // #10798: a genuine `class X extends Stream` subclass is a real
                // ObjectHeader carrying its own class id, chained through the
                // dedicated Stream hop (`class_registry::parent_static`'s
                // `js_register_class_parent_dynamic`) rather than prototype-
                // linked to the real `Stream.prototype` — so it is invisible to
                // `is_classic_stream_instance_of`'s own-field probe above
                // (which answers the DIRECT `new Readable()`-shaped case). Walk
                // the class-id chain the same way the EventEmitter branch below
                // does for its own subclass case.
                || (method == "Stream"
                    && js_instanceof(value, 0xFFFF0070).to_bits() == crate::value::TAG_TRUE))
        {
            return f64::from_bits(crate::value::TAG_TRUE);
        }
        if module == "events" && method == "EventEmitter" {
            // #10556: a genuine subclass instance (`class Sub extends
            // EventEmitter {}`) is a real ObjectHeader carrying Sub's own
            // class id, not a handle and not prototype-linked to the real
            // `EventEmitter.prototype` — so it is invisible to the
            // handle/prototype probes below. Delegate to the static path
            // first: `js_instanceof` walks the class-chain parent edge that
            // codegen registers for `extends EventEmitter`
            // (`builtin_parent_reserved_class_id` in
            // perry-codegen/src/expr/instance_misc1.rs), and its own
            // `CLASS_ID_EVENT_EMITTER` branch already covers the direct
            // handle/`util.inherits` cases. Keep the general prototype walk
            // as a fallback for shapes neither path reaches.
            return f64::from_bits(
                if js_instanceof(value, CLASS_ID_EVENT_EMITTER).to_bits() == crate::value::TAG_TRUE
                    || ordinary_has_instance_prototype_walk(value, type_ref)
                {
                    crate::value::TAG_TRUE
                } else {
                    TAG_FALSE
                },
            );
        }
        if module == "events"
            && method == "EventEmitterAsyncResource"
            && is_event_emitter_async_resource_instance_value(value)
        {
            return f64::from_bits(crate::value::TAG_TRUE);
        }
        if module == "async_hooks"
            && matches!(method.as_str(), "AsyncLocalStorage" | "AsyncResource")
        {
            let raw = value_addr(value);
            let matched = if method == "AsyncResource" {
                crate::async_hooks::resolve_async_resource_handle(raw as i64).is_some()
                    || (crate::value::addr_class::is_plausible_heap_addr(raw)
                        && ordinary_has_instance_prototype_walk(value, type_ref))
            } else {
                let candidate = small_native_handle_id(value).unwrap_or(raw as i64);
                let native = (candidate != 0) && {
                    super::class_handles::handle_property_dispatch().is_some_and(|dispatch| {
                        let property = b"getStore";
                        let result =
                            unsafe { dispatch(candidate, property.as_ptr(), property.len()) };
                        value_is_callable(result)
                    })
                };
                native
                    || (crate::value::addr_class::is_plausible_heap_addr(raw)
                        && ordinary_has_instance_prototype_walk(value, type_ref))
            };
            return f64::from_bits(if matched {
                crate::value::TAG_TRUE
            } else {
                TAG_FALSE
            });
        }
        if module == "tty"
            && matches!(method.as_str(), "ReadStream" | "WriteStream")
            && crate::tty::is_tty_stream_instance(value, method.as_str())
        {
            return f64::from_bits(crate::value::TAG_TRUE);
        }
        if module == "fs" {
            let matched = match method.as_str() {
                "Stats" => crate::fs::is_fs_stats_instance_value(value),
                "Dir" => crate::fs::is_fs_dir_instance_value(value),
                "Dirent" => crate::fs::is_fs_dirent_instance_value(value),
                "ReadStream" | "FileReadStream" | "WriteStream" | "FileWriteStream"
                | "Utf8Stream" => crate::fs::is_fs_stream_instance_value(value, method.as_str()),
                _ => false,
            };
            if matched {
                return f64::from_bits(crate::value::TAG_TRUE);
            }
        }
        if module == "tls"
            && method == "SecureContext"
            && crate::tls::is_secure_context_instance(value)
        {
            return f64::from_bits(crate::value::TAG_TRUE);
        }
        if module == "tls" && matches!(method.as_str(), "Server" | "TLSSocket") {
            let want = if method == "Server" { 1 } else { 2 };
            if let (Some(handle), Some(probe)) = (
                small_native_handle_id(value),
                crate::object::tls_handle_kind_probe(),
            ) {
                return f64::from_bits(if unsafe { probe(handle) } == want {
                    crate::value::TAG_TRUE
                } else {
                    TAG_FALSE
                });
            }
        }
        if module == "wasi" && method == "WASI" && crate::wasi::is_wasi_instance(value) {
            return f64::from_bits(crate::value::TAG_TRUE);
        }
        if module == "repl" {
            let matched = match method.as_str() {
                "Recoverable" => crate::node_repl::is_recoverable_value(value),
                "REPLServer" => crate::node_repl::is_repl_server_value(value),
                _ => false,
            };
            if matched {
                return f64::from_bits(crate::value::TAG_TRUE);
            }
        }
        // #2689: `net.Stream` is an alias for `net.Socket`; both should match
        // a live socket handle via the runtime probe.
        if module == "net" && matches!(method.as_str(), "Socket" | "Stream") {
            if let Some(handle) = small_native_handle_id(value) {
                let net_socket = crate::object::net_socket_handle_probe()
                    .map(|probe| unsafe { probe(handle) })
                    .unwrap_or(false);
                let tls_socket = crate::object::tls_handle_kind_probe()
                    .map(|probe| unsafe { probe(handle) == 2 })
                    .unwrap_or(false);
                if net_socket || tls_socket {
                    return f64::from_bits(crate::value::TAG_TRUE);
                }
            }
        }
        if module == "console"
            && method == "Console"
            && crate::builtins::is_console_instance_value(value)
        {
            return f64::from_bits(crate::value::TAG_TRUE);
        }
        if module == "crypto" && method == "KeyObject" {
            let addr = value_addr(value);
            return if addr != 0
                && (crate::buffer::is_secret_key(addr)
                    || crate::buffer::asymmetric_key_meta(addr).is_some())
            {
                f64::from_bits(crate::value::TAG_TRUE)
            } else {
                f64::from_bits(TAG_FALSE)
            };
        }
        // #11919 P0: a native-payload family answers by its class id.
        if let Some(class_id) = crate::native_payload::export_class_id(&module, &method) {
            let result = js_instanceof(value, class_id);
            if result.to_bits() == crate::value::TAG_TRUE
                || ordinary_has_instance_prototype_walk(value, type_ref)
            {
                return f64::from_bits(crate::value::TAG_TRUE);
            }
            return result;
        }
        if module == "perf_hooks" {
            let class_id = match method.as_str() {
                "Performance" => crate::perf_hooks::CLASS_ID_PERFORMANCE,
                "PerformanceEntry" => crate::perf_hooks::CLASS_ID_PERFORMANCE_ENTRY,
                "PerformanceMark" => crate::perf_hooks::CLASS_ID_PERFORMANCE_MARK,
                "PerformanceMeasure" => crate::perf_hooks::CLASS_ID_PERFORMANCE_MEASURE,
                "PerformanceObserverEntryList" => {
                    crate::perf_hooks::CLASS_ID_PERFORMANCE_OBSERVER_ENTRY_LIST
                }
                "PerformanceResourceTiming" => {
                    crate::perf_hooks::CLASS_ID_PERFORMANCE_RESOURCE_TIMING
                }
                _ => 0,
            };
            if class_id != 0 {
                return js_instanceof(value, class_id);
            }
        }
    }
    if is_buffer_constructor_value(type_ref) {
        return js_instanceof(value, crate::buffer::NODE_BUFFER_CLASS_ID);
    }
    if crate::node_submodules::is_diagnostics_channel_constructor_value(type_ref) {
        return if crate::node_submodules::diagnostics_channel_is_channel_instance_value(value) {
            f64::from_bits(crate::value::TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        };
    }
    // `inst instanceof Intl.<Ctor>`: Intl instances are plain heap objects whose
    // `[[Prototype]]` is `Intl.<Ctor>.prototype` but carry no class-id, so the
    // arms above can't match them. Walk their static-prototype chain.
    // `Intl.*` brand checks. Behind `intl-namespace`: with the feature off no
    // Intl constructor value can exist (the namespace install is a no-op), so
    // the probe could never match — and skipping it keeps this always-live
    // dispatcher from statically pinning every Intl constructor thunk (~204 KB).
    if let Some(is_inst) = crate::intl::hooked::intl_instanceof(value, type_ref) {
        return if is_inst {
            f64::from_bits(crate::value::TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        };
    }
    js_instanceof_dynamic_tail(value, type_ref)
}
