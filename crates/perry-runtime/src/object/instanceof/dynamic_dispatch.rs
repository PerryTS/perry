//! `js_instanceof_dynamic` — the dynamic (runtime-class-ref) form of
//! `instanceof`, resolving a value/class-ref RHS pair rather than a
//! compile-time-known class id.
//!
//! Admit the RHS body once; hooks retain their precedence over the shared
//! class dispatch and native constructor brand/prototype dispatch.

use super::*;

#[derive(Clone, Copy)]
enum RhsKind {
    Ordinary,
    Native,
    Class(u32),
    ClassObject,
    Bound,
    Proxy,
    NonCallable,
}

/// The info word is the authority for a function's dispatch kind. Keep the
/// admitted cell beside that fact; an own-hook getter may move the cell, but
/// cannot change its body identity.
struct InstanceofRhs {
    kind: RhsKind,
    closure: Option<(
        *const crate::closure::ClosureHeader,
        &'static crate::closure::JsFunctionInfo,
    )>,
}

impl InstanceofRhs {
    #[inline]
    fn classify(value: f64) -> Self {
        if crate::proxy_hooks::proxy_is_proxy(value) != 0 {
            return Self {
                kind: RhsKind::Proxy,
                closure: None,
            };
        }
        let jv = crate::JSValue::from_bits(value.to_bits());
        let closure = if jv.is_pointer() {
            let ptr = jv.as_pointer::<crate::closure::ClosureHeader>();
            crate::closure::closure_info(ptr).map(|info| (ptr, info))
        } else {
            None
        };
        let kind = if let Some((ptr, info)) = closure {
            if crate::closure::shape::is_class_info(info) {
                // SAFETY: closure_info admitted this live closure.
                RhsKind::Class(
                    unsafe { crate::object::class_value::class_closure_id_unchecked(ptr) }.unwrap(),
                )
            } else if info.code == crate::closure::BOUND_FUNCTION_FUNC_PTR {
                RhsKind::Bound
            } else if info.flags & crate::codegen_abi::FN_COMPILED_BODY != 0 {
                RhsKind::Ordinary
            } else {
                RhsKind::Native
            }
        } else if let Some(id) = (!jv.is_reference()).then(|| class_ref_id(value)).flatten() {
            RhsKind::Class(id)
        } else if is_class_object_value(value) {
            RhsKind::ClassObject
        } else if crate::value::is_js_handle(value) && crate::value::js_handle_is_function(value) {
            RhsKind::Native
        } else {
            RhsKind::NonCallable
        };
        Self { kind, closure }
    }

    #[inline]
    fn refresh_after_getter(&mut self, value: f64) {
        if let Some((_, info)) = self.closure {
            self.closure = Some((
                crate::JSValue::from_bits(value.to_bits()).as_pointer(),
                info,
            ));
        }
    }
}

#[no_mangle]
pub extern "C" fn js_instanceof_dynamic(value: f64, mut type_ref: f64) -> f64 {
    // Proxy ids are registry handles, not closure headers. Resolve their
    // observable @@hasInstance/prototype reads before any constructor probe
    // or unwrapping of the left operand (#10364).
    let mut rhs = InstanceofRhs::classify(type_ref);
    if matches!(rhs.kind, RhsKind::Proxy) {
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
    let lhs_non_proxy = {
        let mut depth = 0;
        while depth < 16 && crate::proxy_hooks::proxy_is_proxy(value) != 0 {
            value = crate::proxy_hooks::proxy_target(value);
            depth += 1;
        }
        depth < 16
    };
    // Spec step (InstanceofOperator): an OWN user-defined `@@hasInstance`
    // overrides even native constructor brand checks. The native generic hook
    // lives on Function.prototype, so the own-property gate distinguishes an
    // explicit override from that inherited default without recursion.
    let symbols_absent = rhs.closure.is_some_and(|(closure, _)| unsafe {
        let bag = (*closure).props;
        bag.is_null()
            || !crate::object::overflow::shadowed(bag)
                && crate::object::shapes::object_shape_record(bag).is_some_and(|record| {
                    // A legacy dictionary can have an ordinary keyless shape;
                    // its private key list carries no immutable absence proof.
                    record.keys() != 0 && record.proves_no_symbols()
                })
    });
    if !symbols_absent {
        let hi_sym = crate::symbol::well_known_symbol("hasInstance");
        if !hi_sym.is_null() {
            let hi_f64 = f64::from_bits(crate::value::JSValue::pointer(hi_sym as *const u8).bits());
            // A validated function already identifies its own-property holder.
            // Symbols and symbol accessors both occupy that bag's key list;
            // querying the generic owner/accessor/data towers repeats this
            // admission. Class immediates and other representations retain
            // their ordinary symbol query.
            let owns_hook = if let Some((closure, _)) = rhs.closure {
                let bag = unsafe { (*closure).props };
                !bag.is_null()
                    && unsafe { super::shaped_symbols::position(bag, hi_sym as usize).is_some() }
            } else {
                unsafe { crate::symbol::js_object_has_own_symbol(type_ref, hi_f64) }
            };
            if owns_hook {
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
                rhs.refresh_after_getter(type_ref);
                if let HasInstanceOutcome::Result(result) = dispatch_own_has_instance(cb, value) {
                    return result;
                }
            }
        }
    }
    // #6558 sibling: `mod instanceof WebAssembly.Module` for the wasm-host
    // module wrapper. Its `[[Prototype]]` does not reach the namespace ctor's
    // `.prototype`, so brand-check its GC-aware internal wrapper identity.
    //
    // This has to run BEFORE the terminal-shape short-circuit below: the
    // wrapper is an ordinary heap object whose shape answers
    // `PROTO_ID_DEFAULT`, so `terminal_prototype_id` claimed the whole
    // comparison and returned `false` from the `Object.prototype` walk without
    // ever reaching this probe — `addModule instanceof WebAssembly.Module`
    // reported `false` for a module this very runtime had built (#9202). A
    // positive match is still the only thing that short-circuits: a miss
    // returns `None`, so the prototype walk below keeps deciding
    // `WebAssembly.Memory` instances and foreign objects exactly as before.
    if let Some(true) = super::global_this::webassembly_value_ctor_instanceof(value, type_ref) {
        return f64::from_bits(crate::value::TAG_TRUE);
    }
    // Native diagnostics wrappers have ordinary shapes, not constructor-linked prototypes.
    if crate::node_submodules::is_diagnostics_channel_constructor_value(type_ref) {
        return if crate::node_submodules::diagnostics_channel_is_channel_instance_value(value) {
            f64::from_bits(crate::value::TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        };
    }
    if crate::node_submodules::is_diagnostics_bounded_channel_constructor_value(type_ref) {
        return if crate::node_submodules::diagnostics_bounded_channel_is_instance_value(value) {
            f64::from_bits(crate::value::TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        };
    }
    // Asymmetric KeyObjects use marked heap strings, not ObjectHeader values.
    // Recognize only this native constructor before ordinary primitive checks;
    // user-defined @@hasInstance above still has first refusal.
    if matches!(rhs.kind, RhsKind::Native) {
        if let Some((module, method)) = unsafe { bound_native_callable_module_and_method(type_ref) }
        {
            if module == "crypto" && method == "KeyObject" {
                let lhs = crate::JSValue::from_bits(value.to_bits());
                let matched = if lhs.is_string() {
                    // is_string admits only heap STRING_TAG, never inline text.
                    // The existing registry owns this brand; do not dereference
                    // a string as a BufferHeader or relax object_ref_addr.
                    let addr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
                    crate::buffer::asymmetric_key_meta(addr).is_some()
                } else {
                    let addr = value_addr(value);
                    addr != 0
                        && (crate::buffer::is_secret_key(addr)
                            || crate::buffer::asymmetric_key_meta(addr).is_some())
                };
                return f64::from_bits(if matched {
                    crate::value::TAG_TRUE
                } else {
                    TAG_FALSE
                });
            }
        }
    }
    // Hooks precede OrdinaryHasInstance. A class must still consult its
    // lifted/static hooks before using this primitive fact.
    let primitive = instanceof_lhs_is_primitive(value);
    if matches!(rhs.kind, RhsKind::Ordinary | RhsKind::Native) {
        // Only non-class functions use the terminal receiver proof. Bound
        // functions have already forwarded, and non-callable values still throw.
        if let Some((closure, _)) = rhs.closure {
            if primitive {
                return f64::from_bits(TAG_FALSE);
            }
            if let Some(terminal) = terminal_prototype_id(value) {
                if let Some(Some(prototype)) =
                    unsafe { crate::closure::shape::closure_own_prototype_by_shape(closure) }
                {
                    if let Some(matches) = terminal_prototype_matches_id(terminal, prototype) {
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
    if matches!(rhs.kind, RhsKind::Bound) {
        let (closure, _) = rhs.closure.unwrap();
        let target = crate::closure::js_closure_get_capture_f64(closure, 0);
        return js_instanceof_dynamic(value, target);
    }
    // `class_ref_id` requires `is_class_id_registered`, not just the tag —
    // a user-crafted NaN payload sharing the 0x7FFE band (a real JS number
    // constructed via `DataView.setFloat64`, not a codegen-emitted class
    // ref) must fall through to the unresolved-RHS `TypeError` below
    // instead of being dispatched into `js_instanceof` as a bogus class id.
    if let RhsKind::Class(class_id) = rhs.kind {
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
        return super::static_dispatch::instanceof_classified(
            value,
            class_id,
            primitive,
            lhs_non_proxy,
        );
    }
    // #9502: a heap class object's template id identifies its code, not its
    // evaluation. Compare the actual prototype objects so sibling evaluations
    // remain distinct and a chain through earlier evaluations still matches.
    if matches!(rhs.kind, RhsKind::ClassObject) {
        // A receiver with a CLASS prototype identity still has a real chain:
        // its declaration prototype can inherit from this exact evaluation.
        // The RHS template id names code, not this evaluation's prototype;
        // substituting it loses that edge and conflates sibling evaluations.
        return f64::from_bits(if ordinary_has_instance_prototype_walk(value, type_ref) {
            crate::value::TAG_TRUE
        } else {
            TAG_FALSE
        });
    }
    if matches!(rhs.kind, RhsKind::Native) {
        // `temporalValue instanceof Temporal.<X>` — Temporal values dispatch via
        // brand arms (not a real prototype chain), so resolve the constructor to
        // its kind and compare against the value's brand. A non-Temporal value, or
        // a Temporal value of a different kind, yields `false`.
        if let Some(kind) = rhs
            .closure
            .and_then(|(_, info)| crate::temporal::hooked::ctor_kind(info.code))
        {
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
    }
    // Class values take their existing exits above. An intrinsic's body
    // identity also proves it is callable and not a bind wrapper, so dispatch
    // it before the remaining callable/bound/native-export probes. Its own
    // @@hasInstance already had first refusal; the shared native dispatch owns
    // the primitive and prototype checks.
    if let Some(declaration) = matches!(rhs.kind, RhsKind::Native)
        .then_some(rhs.closure)
        .flatten()
        .and_then(|(closure, info)| unsafe {
            super::super::global_this_tables::declaration_of_constructor(closure, info)
        })
    {
        use super::super::global_this_tables::BuiltinInstanceIdentity;
        let matched = match declaration.instance_identity {
            Some(BuiltinInstanceIdentity::Class(class_id)) => {
                return super::static_dispatch::native_with_constructor(
                    value,
                    class_id,
                    Some(type_ref),
                );
            }
            Some(BuiltinInstanceIdentity::Crypto) => {
                is_native_module_namespace_value(value, "crypto.webcrypto")
            }
            Some(BuiltinInstanceIdentity::SubtleCrypto) => {
                is_native_module_namespace_value(value, "crypto.subtle")
            }
            Some(BuiltinInstanceIdentity::CryptoKey) => {
                let addr = value_addr(value);
                addr != 0 && crate::buffer::crypto_key_meta(addr).is_some()
            }
            None => false,
        };
        if declaration.instance_identity.is_some() {
            return f64::from_bits(if matched {
                crate::value::TAG_TRUE
            } else {
                TAG_FALSE
            });
        }
    }
    // Intrinsics and class values have completed their own dispatch. Only
    // remaining function values need the compiled-function ancestry probe;
    // ordinary native constructor checks no longer pay its failed validation.
    // #10507: `x instanceof F` for an ordinary compiled function `F`: one
    // shape compare when `x`'s ShapeId names `F.prototype`, else, for an
    // object of no compiled class, OrdinaryHasInstance's prototype walk.
    if matches!(rhs.kind, RhsKind::Ordinary) {
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
    if primitive && !matches!(rhs.kind, RhsKind::NonCallable) {
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
    if let Some((module, method)) = unsafe { bound_native_callable_module_and_method(type_ref) } {
        if module == "stream"
            && matches!(
                method.as_str(),
                "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough" | "Stream"
            )
            && (crate::stream_hooks::is_classic_stream_instance_of(value, method.as_str())
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
                    && super::static_dispatch::instanceof_classified(value, 0xFFFF0070, primitive, lhs_non_proxy).to_bits() == crate::value::TAG_TRUE))
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
                if super::static_dispatch::instanceof_classified(
                    value,
                    CLASS_ID_EVENT_EMITTER,
                    primitive,
                    lhs_non_proxy,
                )
                .to_bits()
                    == crate::value::TAG_TRUE
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
                crate::async_hook_slots::resolve_async_resource_handle(raw as i64).is_some()
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
                | "Utf8Stream" => {
                    crate::fs_hook_slots::is_fs_stream_instance_value(value, method.as_str())
                }
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
        if module == "wasi" && method == "WASI" && crate::wasi_hook_slots::is_wasi_instance(value) {
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
        // #11919 P0: a native-payload family answers by its class id.
        if let Some(class_id) = crate::native_payload::export_class_id(&module, &method) {
            let result = super::static_dispatch::instanceof_classified(
                value,
                class_id,
                primitive,
                lhs_non_proxy,
            );
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
                return super::static_dispatch::instanceof_classified(
                    value,
                    class_id,
                    primitive,
                    lhs_non_proxy,
                );
            }
        }
    }
    if is_buffer_constructor_value(type_ref) {
        return super::static_dispatch::instanceof_classified(
            value,
            crate::buffer::NODE_BUFFER_CLASS_ID,
            primitive,
            lhs_non_proxy,
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subclass_instanceof_uses_the_evaluated_parent_prototype() {
        let _lock = crate::gc::global_side_table_test_lock();
        const PARENT: u32 = 0x6B4B;
        const CHILD: u32 = 0x6B4C;
        unsafe {
            crate::object::js_register_class_id(PARENT);
            crate::object::js_register_class_id(CHILD);
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let parent = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_class_evaluation_object(PARENT, 6, 0, std::ptr::null()),
        ));
        let other = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_class_evaluation_object(PARENT, 6, 0, std::ptr::null()),
        ));
        // Both values name the same code template, but distinct evaluations.
        assert_eq!(
            crate::object::class_registry::dynamic_value_class_id(parent.get_nanbox_f64()),
            PARENT
        );
        assert_eq!(
            crate::object::class_registry::dynamic_value_class_id(other.get_nanbox_f64()),
            PARENT
        );
        crate::object::js_register_class_parent_dynamic(CHILD, parent.get_nanbox_f64());
        let instance = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(CHILD, 0) as i64,
        ));
        assert!(
            crate::object::prototype_chain::object_static_prototype(value_addr(
                instance.get_nanbox_f64()
            ))
            .is_none()
        );
        assert_eq!(
            js_instanceof_dynamic(instance.get_nanbox_f64(), parent.get_nanbox_f64()).to_bits(),
            crate::value::TAG_TRUE
        );
        assert_eq!(
            js_instanceof_dynamic(instance.get_nanbox_f64(), other.get_nanbox_f64()).to_bits(),
            crate::value::TAG_FALSE
        );
    }
}
