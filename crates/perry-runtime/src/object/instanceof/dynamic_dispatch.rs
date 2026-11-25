//! `js_instanceof_dynamic` — the dynamic (runtime-class-ref) form of
//! `instanceof`, resolving a value/class-ref RHS pair rather than a
//! compile-time-known class id.
//!
//! Admit the RHS body once; hooks retain their precedence over the shared
//! class dispatch and native constructor brand/prototype dispatch.

use super::*;
use crate::object::overflow::home::ObjHome;

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
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    // A typed array against the realm's typed-array constructor, answered from
    // structure before any other classification (none of which concerns them).
    if let Some(matched) = unsafe { typed_array_instanceof_front(value, type_ref) } {
        return f64::from_bits(if matched {
            crate::value::TAG_TRUE
        } else {
            TAG_FALSE
        });
    }
    // Proxy ids are registry handles, not closure headers. Resolve their
    // observable @@hasInstance/prototype reads before any constructor probe
    // or unwrapping of the left operand (#10364).
    let mut rhs = InstanceofRhs::classify(type_ref);
    if matches!(rhs.kind, RhsKind::Proxy) {
        return super::proxy_rhs::proxy_instanceof(value, type_ref);
    }
    // The realm's typed-array constructor whose `@@hasInstance` is the default
    // one: OrdinaryHasInstance from structure, before every probe below (none
    // of which concerns this constructor).
    if let (RhsKind::Native, Some((closure, info))) = (rhs.kind, rhs.closure) {
        if let Some(matched) = unsafe { typed_array_constructor_instanceof(value, closure, info) } {
            return f64::from_bits(if matched {
                crate::value::TAG_TRUE
            } else {
                TAG_FALSE
            });
        }
    }
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
    // That inherited default is the hook only while the function's chain is
    // its intrinsic one: a linked function's `@@hasInstance` is GetMethod's
    // full [[Get]] over its chain (the intrinsic thunk, reached, answers the
    // same ordinary walk).
    let linked = rhs.closure.is_some_and(|(closure, _)| unsafe {
        linked_chain_may_hook(
            closure,
            crate::symbol::well_known_symbol("hasInstance") as usize,
        )
    });
    let symbols_absent = !linked
        && rhs.closure.is_some_and(|(closure, _)| unsafe {
            crate::closure::props::proves_no_symbols(closure as usize)
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
                linked
                    || unsafe {
                        crate::closure::props::owns_symbol(closure as usize, hi_sym as usize)
                    }
            } else {
                unsafe { crate::symbol::js_object_has_own_symbol(type_ref, hi_f64) }
            };
            if owns_hook {
                // An own @@hasInstance getter can collect even when it returns
                // undefined and ordinary dispatch continues: the hook read
                // refreshes both operands.
                if let HasInstanceOutcome::Result(result) =
                    dispatch_own_has_instance(OwnHook::Read(hi_f64), &mut type_ref, &mut value)
                {
                    return result;
                }
                rhs.refresh_after_getter(type_ref);
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
    // Class identity comes from the constructor function object. Numeric
    // INT32 words never supply an instanceof RHS class id.
    if let RhsKind::Class(class_id) = rhs.kind {
        // #11759 (c′): the class function object is the first evaluation of a
        // declaration whose later evaluations share its template id. A value
        // with an individually recorded prototype chain (an instance of a
        // later evaluation) answers by its actual chain.
        if crate::object::class_value::class_value_is_first_evaluation(class_id)
            && super::prototype_chain::has_recorded_prototype(value_addr(value))
        {
            return f64::from_bits(if ordinary_has_instance_prototype_walk(value, type_ref) {
                crate::value::TAG_TRUE
            } else {
                TAG_FALSE
            });
        }
        // A class function object had its `@@hasInstance` resolved above on
        // its actual chain (`linked_chain_may_hook`, then the complete read).
        // A class-ref immediate had only its own keys asked.
        if rhs.closure.is_some() {
            return super::static_dispatch::instanceof_class_resolved(
                value,
                class_id,
                primitive,
                lhs_non_proxy,
            );
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
            // recover that cell and compare its kind. (#5587)
            {
                let bits = value.to_bits();
                let top16 = bits >> 48;
                let raw = if top16 == 0x7FFD {
                    (bits & crate::value::POINTER_MASK) as usize
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
                if let Some(matched) = unsafe {
                    crate::object::typed_array_chain::typed_array_has_instance(value, class_id)
                } {
                    return f64::from_bits(if matched {
                        crate::value::TAG_TRUE
                    } else {
                        TAG_FALSE
                    });
                }
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
    // Live http(s) servers are registry handles too: same brand check.
    if let Some((module, "Server")) = unsafe { bound_native_callable_module_and_method(type_ref) }
        .as_ref()
        .map(|(module, method)| (module.as_str(), method.as_str()))
    {
        let want = match module {
            "http" => Some(super::class_handles::HTTP_SERVER_KIND),
            "https" => Some(super::class_handles::HTTPS_SERVER_KIND),
            _ => None,
        };
        if want.is_some()
            && small_native_handle_id(value)
                .zip(super::class_handles::http_server_handle_kind_probe())
                .map(|(handle, probe)| unsafe { probe(handle) })
                == want
        {
            return f64::from_bits(crate::value::TAG_TRUE);
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

/// `value instanceof C` for `C` the realm's typed-array constructor `closure`
/// (its intrinsic body and identity, `declaration_of_constructor`), when
/// GetMethod(C, @@hasInstance) is `%Function.prototype%`'s
/// (`Function.prototype[@@hasInstance]` is neither writable nor
/// configurable): `C` owns no `@@hasInstance` and no hop of its linked chain
/// before `%Function.prototype%` does. That method is OrdinaryHasInstance(C,
/// value), and `C.prototype` (neither writable nor configurable) is its kind's
/// intrinsic prototype: `typed_array_chain::typed_array_constructor_has_instance`
/// answers it from structure. `None` for every other case.
///
/// # Safety
/// `closure` is a live closure cell and `info` its admitted info.
unsafe fn typed_array_constructor_instanceof(
    value: f64,
    closure: *const crate::closure::ClosureHeader,
    info: &'static crate::closure::JsFunctionInfo,
) -> Option<bool> {
    use super::super::global_this_tables::BuiltinInstanceIdentity;
    let declaration = super::super::global_this_tables::declaration_of_constructor(closure, info)?;
    let Some(BuiltinInstanceIdentity::Class(class_id)) = declaration.instance_identity else {
        return None;
    };
    let first = crate::typedarray_hook_slots::CLASS_ID_INT8_ARRAY;
    let last = crate::typedarray_hook_slots::CLASS_ID_FLOAT16_ARRAY;
    if !(first..=last).contains(&class_id) {
        return None;
    }
    let hi_sym = crate::symbol::well_known_symbol("hasInstance") as usize;
    if hi_sym == 0 || !bag_lacks_symbol((*closure).props, hi_sym) {
        return None;
    }
    if linked_chain_may_hook(closure, hi_sym) {
        return None;
    }
    crate::object::typed_array_chain::typed_array_constructor_has_instance(value, class_id)
}

/// Does the property bag `bag` (null: none) own no `hi_sym` key? From its
/// shape (no symbol key at all) or its key list (no entry for that symbol).
///
/// # Safety
/// `bag` is null or a live `GC_TYPE_OBJECT` cell.
#[inline]
unsafe fn bag_lacks_symbol(bag: *mut ObjectHeader, hi_sym: usize) -> bool {
    bag.is_null()
        || !crate::object::overflow::shadowed(bag)
            && crate::object::shapes::object_shape_record(bag)
                .is_some_and(|record| record.keys() != 0 && record.proves_no_symbols())
        || super::shaped_symbols::position(bag, hi_sym).is_none()
}

/// `value instanceof C` (ECMA-262 13.10.2 InstanceofOperator) for `value` a
/// typed array and `C` the realm's typed-array constructor of some kind:
///
/// * `C` is a function cell whose immutable constructor identity
///   (`declaration_of_constructor`) names a typed-array kind;
/// * GetMethod(C, @@hasInstance) is `%Function.prototype%[@@hasInstance]`
///   (neither writable nor configurable): `C` owns no `@@hasInstance` and no
///   hop of its linked chain before `%Function.prototype%` does. That method
///   is OrdinaryHasInstance(C, value), and `C.prototype` (neither writable nor
///   configurable) is the kind's intrinsic prototype;
/// * `typed_array_chain::typed_array_has_instance` answers that from the
///   value's structure when its chain is the default one.
///
/// `None` for every other operand pair or chain: the complete operation.
/// Reads no property through a getter and runs no code.
///
/// # Safety
/// Both operands are any NaN-boxed values.
#[inline]
unsafe fn typed_array_instanceof_front(value: f64, type_ref: f64) -> Option<bool> {
    use crate::value::{POINTER_MASK, POINTER_TAG};
    let (vb, rb) = (value.to_bits(), type_ref.to_bits());
    if vb & !POINTER_MASK != POINTER_TAG || rb & !POINTER_MASK != POINTER_TAG {
        return None;
    }
    // The left operand first: the cheap refusal for every value that is no
    // typed array (one header read).
    let vaddr = (vb & POINTER_MASK) as usize;
    let vty = crate::value::addr_class::try_read_gc_header(vaddr)?.obj_type;
    if !crate::gc::is_typed_array_type(vty) {
        return None;
    }
    let closure = (rb & POINTER_MASK) as *const crate::closure::ClosureHeader;
    let info = crate::closure::closure_info(closure)?;
    use super::super::global_this_tables::BuiltinInstanceIdentity;
    let declaration = super::super::global_this_tables::declaration_of_constructor(closure, info)?;
    let Some(BuiltinInstanceIdentity::Class(class_id)) = declaration.instance_identity else {
        return None;
    };
    let first = crate::typedarray_hook_slots::CLASS_ID_INT8_ARRAY;
    let last = crate::typedarray_hook_slots::CLASS_ID_FLOAT16_ARRAY;
    if !(first..=last).contains(&class_id) {
        return None;
    }
    // GetMethod(C, @@hasInstance): C's own key list, then its linked chain.
    let hi_sym = crate::symbol::well_known_symbol("hasInstance") as usize;
    if hi_sym == 0 {
        return None;
    }
    if !bag_lacks_symbol((*closure).props, hi_sym) || linked_chain_may_hook(closure, hi_sym) {
        return None;
    }
    crate::object::typed_array_chain::typed_array_has_instance(value, class_id)
}

/// Can the chain a linked function `closure` names supply an `@@hasInstance`
/// other than `%Function.prototype%`'s default? Walks the hops before that
/// default (or null): each must prove it owns no `@@hasInstance` (`hi_sym`),
/// from its shape (no symbol key at all) or its key list (no entry for that
/// symbol, the test the receiver's own bag takes). `false` for a function on
/// its intrinsic chain, or one whose linked chain proves absence hop by hop
/// (a builtin constructor linked to its parent: `Float32Array` to
/// `%TypedArray%`, which owns `@@species`).
///
/// # Safety
/// `closure` is a proven, live closure cell; `hi_sym` is the
/// `Symbol.hasInstance` header, or 0.
unsafe fn linked_chain_may_hook(
    closure: *const crate::closure::ClosureHeader,
    hi_sym: usize,
) -> bool {
    let Some(mut bits) = crate::closure::closure_static_prototype(closure as usize) else {
        return false;
    };
    // `%Function.prototype%` itself, by identity (no by-name realm lookup).
    let fp = crate::value::js_nanbox_pointer(crate::array::intrinsic_prototype_addr(
        crate::object::BuiltinConstructorIdentity::Function,
    ) as i64)
    .to_bits();
    for _ in 0..32 {
        let v = crate::JSValue::from_bits(bits);
        if bits == fp || v.is_null() {
            return false;
        }
        if !v.is_pointer() {
            return true;
        }
        let addr = v.as_pointer::<u8>() as usize;
        let proves = |bag: *mut ObjectHeader| {
            bag.is_null()
                || ObjHome::proves_no_symbols(bag as usize)
                || hi_sym != 0 && ObjHome::own_symbol_position(bag as usize, hi_sym).is_none()
        };
        if crate::closure::is_closure_ptr(addr) {
            let hop = addr as *const crate::closure::ClosureHeader;
            // A function's own `@@hasInstance` is a key of its bag. A class
            // hop may also hold one where `[[Get]]` on the function does not
            // read it: its class's static-symbol table or its lifted
            // `static [Symbol.hasInstance]` hook. Such a hop takes the full
            // read and the class dispatch, as before; a hop with neither is
            // answered by its bag like any function.
            if crate::closure::shape::is_class_info((*hop).info) && class_hop_may_hook(hop, hi_sym)
            {
                return true;
            }
            if !(crate::closure::props::proves_no_symbols(hop as usize)
                || hi_sym != 0 && !crate::closure::props::owns_symbol(hop as usize, hi_sym))
            {
                return true;
            }
            bits = match crate::closure::closure_static_prototype(addr) {
                Some(next) => next,
                None => return false,
            };
            continue;
        }
        match crate::value::addr_class::try_read_gc_header(addr) {
            Some(header)
                if header.obj_type == crate::gc::GC_TYPE_OBJECT
                    && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0 =>
            {
                if !proves(addr as *mut ObjectHeader) {
                    return true;
                }
                bits = match crate::object::prototype_chain::object_static_prototype(addr) {
                    Some(next) => next,
                    None => crate::object::builtin_prototype_value("Object").to_bits(),
                };
            }
            _ => return true,
        }
    }
    true
}

/// May the class constructor `hop` hold an `@@hasInstance` outside its own
/// bag: an entry of its class's static-symbol table (a computed static
/// field, a static accessor, `Object.defineProperty(C, Symbol.hasInstance,
/// …)`), or a lifted `static [Symbol.hasInstance]` method registered for its
/// class id or a parent class id? Both registries are latched: an idle one
/// answers with one load. A class whose id cannot be read may.
///
/// # Safety
/// `hop` is a proven, live class constructor closure.
unsafe fn class_hop_may_hook(hop: *const crate::closure::ClosureHeader, hi_sym: usize) -> bool {
    if hi_sym == 0 {
        return true;
    }
    let Some(class_id) = crate::object::class_value::class_closure_id_unchecked(hop) else {
        return true;
    };
    let hi_f64 = f64::from_bits(crate::value::JSValue::pointer(hi_sym as *const u8).bits());
    crate::symbol::class_static_symbol_lookup(class_id, hi_f64).is_some()
        || crate::object::lookup_has_instance_hook(class_id).is_some()
}
