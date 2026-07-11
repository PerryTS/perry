//! `instanceof` evaluation: `js_instanceof` and the dynamic
//! (runtime-class-ref) form `js_instanceof_dynamic`.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation.

use super::*;

// Keep in sync with perry-codegen/src/expr/instance_misc1.rs.
const CLASS_ID_EVENT_EMITTER: u32 = 0xFFFF0076;
const CLASS_ID_EVENT_EMITTER_ASYNC_RESOURCE: u32 = 0xFFFF0077;
const CLASS_ID_PROMISE: u32 = 0xFFFF0027;
const CLASS_ID_NET_SOCKET: u32 = 0xFFFF00B4;
const CLASS_ID_CRYPTO: u32 = 0xFFFF00C0;
const CLASS_ID_SUBTLE_CRYPTO: u32 = 0xFFFF00C1;
const CLASS_ID_CRYPTO_KEY: u32 = 0xFFFF00C2;
/// `value instanceof Function` reserved id (see `js_instanceof`).
const CLASS_ID_FUNCTION: u32 = 0xFFFF00F0;

/// Whether `value` is callable — the predicate behind `x instanceof Function`
/// and `Function[Symbol.hasInstance]`. Covers every Perry function
/// representation: heap closures (declarations / expressions / arrows /
/// methods / bound functions / built-in constructors, all carrying
/// `CLOSURE_MAGIC`) and small native function handles.
pub(crate) fn value_is_callable(value: f64) -> bool {
    if crate::value::is_js_handle(value) && crate::value::js_handle_is_function(value) {
        return true;
    }
    // INT32-tagged class references (top 16 bits = 0x7FFE) are callable
    // constructors emitted by codegen. `is_pointer()` only checks 0x7FFD,
    // so they would fall through to `return false` without this guard.
    if (value.to_bits() >> 48) == 0x7FFE {
        return true;
    }
    let jv = crate::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    crate::closure::is_closure_ptr((jv.bits() & crate::value::POINTER_MASK) as usize)
}

fn small_native_handle_id(value: f64) -> Option<i64> {
    use crate::value::addr_class;
    let bits = value.to_bits();
    if (bits & crate::value::TAG_MASK) == crate::value::POINTER_TAG {
        let raw = (bits & crate::value::POINTER_MASK) as i64;
        if addr_class::is_small_handle(raw as usize) {
            return Some(raw);
        }
    }
    if addr_class::is_small_handle(bits as usize) {
        return Some(bits as i64);
    }
    if value.is_finite()
        && value > 0.0
        && value.fract() == 0.0
        && value < addr_class::HANDLE_BAND_MAX as f64
    {
        return Some(value as i64);
    }
    None
}

fn value_addr(value: f64) -> usize {
    let bits = value.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        (bits & crate::value::POINTER_MASK) as usize
    } else if (bits >> 48) == 0 && bits >= 0x1000 {
        bits as usize
    } else {
        0
    }
}

fn is_native_module_namespace_value(value: f64, expected: &str) -> bool {
    let jv = crate::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let obj = jv.as_pointer::<ObjectHeader>();
    if obj.is_null() {
        return false;
    }
    unsafe {
        (*obj).class_id == crate::object::native_module::NATIVE_MODULE_CLASS_ID
            && crate::object::native_module::read_native_module_name(obj)
                .is_some_and(|name| name == expected)
    }
}

/// v0.5.749: dynamic instanceof — `value instanceof type` where the
/// type is a runtime value (function arg holding a class ref). Extracts
/// the class_id from the INT32 NaN-tag (top16=0x7FFE) and dispatches to
/// `js_instanceof`. Returns FALSE for non-class-ref type values (matches
/// JS spec: `1 instanceof 2` throws, but Perry returns false defensively).
/// Refs #420 / #618 followup.
#[no_mangle]
pub extern "C" fn js_instanceof_dynamic(value: f64, type_ref: f64) -> f64 {
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
        #[cfg(feature = "temporal")]
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
                if let Some(cell) = unsafe { crate::object::temporal_subclass_cell(raw) } {
                    if crate::temporal::temporal_kind(cell) == Some(kind) {
                        return f64::from_bits(crate::value::TAG_TRUE);
                    }
                }
            }
        }
        return f64::from_bits(TAG_FALSE);
    }
    // Spec step (InstanceofOperator): consult the RHS's OWN `@@hasInstance`
    // first. zod 4 builds `ZodType` as a plain FUNCTION and installs its brand
    // check via `Object.defineProperty(ZodTypeFn, Symbol.hasInstance, { value })`,
    // so the function RHS would otherwise fall through to the prototype-walk tail
    // and return false. Read OWN only (`has_own` ⇒ `get` returns the own value,
    // never the inherited Function.prototype default thunk → no recursion). Also
    // catches a dynamic class-ref RHS (own @@hasInstance lives in CLASS_STATIC_SYMBOLS).
    {
        let hi_sym = crate::symbol::well_known_symbol("hasInstance");
        if !hi_sym.is_null() {
            let hi_f64 = f64::from_bits(crate::value::JSValue::pointer(hi_sym as *const u8).bits());
            if unsafe { crate::symbol::js_object_has_own_symbol(type_ref, hi_f64) } {
                let cb = unsafe { crate::symbol::js_object_get_symbol_property(type_ref, hi_f64) };
                // A present-but-non-callable own `@@hasInstance` throws; only a
                // `null`/`undefined` value falls through to the default below.
                if let HasInstanceOutcome::Result(r) = dispatch_own_has_instance(cb, value) {
                    return r;
                }
            }
        }
    }
    // Spec step (InstanceofOperator): consult the RHS's OWN `@@hasInstance`
    // first. zod 4 builds `ZodType` as a plain FUNCTION and installs its brand
    // check via `Object.defineProperty(ZodTypeFn, Symbol.hasInstance, { value })`,
    // so the function RHS would otherwise fall through to the prototype-walk tail
    // and return false. Read OWN only (`has_own` ⇒ `get` returns the own value,
    // never the inherited Function.prototype default thunk → no recursion). Also
    // catches a dynamic class-ref RHS (own @@hasInstance lives in CLASS_STATIC_SYMBOLS).
    {
        let hi_sym = crate::symbol::well_known_symbol("hasInstance");
        if !hi_sym.is_null() {
            let hi_f64 = f64::from_bits(crate::value::JSValue::pointer(hi_sym as *const u8).bits());
            if unsafe { crate::symbol::js_object_has_own_symbol(type_ref, hi_f64) } {
                let cb = unsafe { crate::symbol::js_object_get_symbol_property(type_ref, hi_f64) };
                // A present-but-non-callable own `@@hasInstance` throws; only a
                // `null`/`undefined` value falls through to the default below.
                if let HasInstanceOutcome::Result(r) = dispatch_own_has_instance(cb, value) {
                    return r;
                }
            }
        }
    }
    let bits = type_ref.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if class_id != 0 {
            return js_instanceof(value, class_id);
        }
    }
    // #1789: `x instanceof C` where C is a heap class object (the value a
    // class EXPRESSION evaluates to, e.g. `const C = make(x); c instanceof
    // C`). Read its class_id (the compile-time template) and walk the
    // candidate's class chain against it.
    if is_class_object_value(type_ref) {
        let obj = crate::JSValue::from_bits(bits).as_pointer::<ObjectHeader>();
        let class_id = js_object_get_class_id(obj);
        if class_id != 0 {
            return js_instanceof(value, class_id);
        }
    }
    if let Some((module, method)) = unsafe { bound_native_callable_module_and_method(type_ref) } {
        if module == "stream"
            && matches!(
                method.as_str(),
                "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough" | "Stream"
            )
            && crate::node_stream::is_classic_stream_instance_of(value, method.as_str())
        {
            return f64::from_bits(crate::value::TAG_TRUE);
        }
        if module == "events" && method == "EventEmitter" && is_event_emitter_instance_value(value)
        {
            return f64::from_bits(crate::value::TAG_TRUE);
        }
        if module == "events"
            && method == "EventEmitterAsyncResource"
            && is_event_emitter_async_resource_instance_value(value)
        {
            return f64::from_bits(crate::value::TAG_TRUE);
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
            if let (Some(handle), Some(probe)) = (
                small_native_handle_id(value),
                crate::object::net_socket_handle_probe(),
            ) {
                if unsafe { probe(handle) } {
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
        return js_instanceof(value, crate::buffer::BUFFER_TYPE_ID);
    }
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
            let r = js_instanceof(value, class_id);
            if r.to_bits() == crate::value::TAG_TRUE {
                return r;
            }
            // #5989: an object that inherits a builtin's prototype via
            // `Fn.prototype = Object.create(Builtin.prototype)` is `instanceof
            // Builtin` per the spec even though it carries no builtin class id —
            // react-server-dom's flight Chunk inherits `Promise.prototype` this
            // way, so `chunk instanceof Promise` must be true. Walk the real
            // [[Prototype]] chain against `Builtin.prototype` before answering
            // false.
            if ordinary_has_instance_prototype_walk(value, type_ref) {
                return f64::from_bits(crate::value::TAG_TRUE);
            }
            return f64::from_bits(TAG_FALSE);
        }
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
    if let Some(is_inst) = crate::intl::intl_instanceof(value, type_ref) {
        return if is_inst {
            f64::from_bits(crate::value::TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        };
    }
    js_instanceof_dynamic_tail(value, type_ref)
}

/// Runtime class id for a globalThis built-in constructor *name*.
///
/// Reference-type global constructors used as runtime values (e.g.
/// `Function.prototype[Symbol.hasInstance].call(Map, m)`, or a dynamic
/// `x instanceof ctorVar`). These mirror the synthetic ids the compile-time
/// `instanceof` operator emits — see perry-codegen/src/expr/instance_misc1.rs
/// — which `js_instanceof` resolves via the per-type registries (#3662).
/// `Array`/`Object`/`Date` carry their own coercion thunks rather than the
/// shared noop thunk; #4102 added those thunks to the
/// `identify_global_builtin_constructor` allow-list so the dynamic /
/// reflective path resolves them just like the literal-RHS operator does at
/// compile time. Also consulted by `js_register_class_parent_dynamic` so a
/// user `class X extends Event` registers the `X → Event` chain edge.
/// Returns 0 for names without a runtime class id.
pub(crate) fn global_builtin_constructor_class_id(name: &str) -> u32 {
    match name {
        "Map" => 0xFFFF0022,
        "Set" => 0xFFFF0023,
        // #5834: kept in sync with the reserved ids in
        // perry-codegen/src/expr/instance_misc1.rs so a dynamic
        // `x instanceof ctorVar` (ctorVar holding WeakMap/WeakSet) resolves
        // through the same runtime probe as the compile-time-literal form.
        "WeakMap" => 0xFFFF002C,
        "WeakSet" => 0xFFFF002D,
        "RegExp" => 0xFFFF0021,
        "ArrayBuffer" => 0xFFFF0025,
        "Array" => 0xFFFF0024,
        "Object" => 0xFFFF0050,
        "Function" => CLASS_ID_FUNCTION,
        "Number" => 0xFFFF00D0,
        "String" => 0xFFFF00D1,
        "Boolean" => 0xFFFF00D2,
        "BigInt" => 0xFFFF00D3,
        "Symbol" => 0xFFFF00D4,
        "Date" => 0xFFFF0020,
        "Error" => crate::error::CLASS_ID_ERROR,
        "TypeError" => crate::error::CLASS_ID_TYPE_ERROR,
        "RangeError" => crate::error::CLASS_ID_RANGE_ERROR,
        "ReferenceError" => crate::error::CLASS_ID_REFERENCE_ERROR,
        "SyntaxError" => crate::error::CLASS_ID_SYNTAX_ERROR,
        "EvalError" => crate::error::CLASS_ID_EVAL_ERROR,
        "URIError" => crate::error::CLASS_ID_URI_ERROR,
        "AggregateError" => crate::error::CLASS_ID_AGGREGATE_ERROR,
        "Promise" => CLASS_ID_PROMISE,
        "Navigator" => crate::navigator::NAVIGATOR_CLASS_ID,
        "TextEncoderStream" => crate::object::CLASS_ID_TEXT_ENCODER_STREAM,
        "TextDecoderStream" => crate::object::CLASS_ID_TEXT_DECODER_STREAM,
        "CompressionStream" => crate::object::CLASS_ID_COMPRESSION_STREAM,
        "DecompressionStream" => crate::object::CLASS_ID_DECOMPRESSION_STREAM,
        "Event" => crate::event_target::CLASS_ID_EVENT,
        "CustomEvent" => crate::event_target::CLASS_ID_CUSTOM_EVENT,
        "DOMException" => crate::event_target::CLASS_ID_DOM_EXCEPTION,
        // TypedArray constructors used as runtime *values* (a dynamic
        // `x instanceof TA` where `TA` is a variable — e.g. test262's
        // `testWithTypedArrayConstructors`). Mirrors the per-kind synthetic
        // ids the compile-time `instanceof Float64Array` operator resolves.
        "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
        | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
        | "BigInt64Array" | "BigUint64Array" => crate::typedarray::kind_for_name(name)
            .map(crate::typedarray::class_id_for_kind)
            .unwrap_or(0),
        _ => 0,
    }
}

#[inline]
fn js_instanceof_dynamic_tail(value: f64, type_ref: f64) -> f64 {
    use crate::value::TAG_FALSE;
    if crate::node_submodules::is_diagnostics_bounded_channel_constructor_value(type_ref) {
        return if crate::node_submodules::diagnostics_bounded_channel_is_instance_value(value) {
            f64::from_bits(crate::value::TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        };
    }
    // ES5 function constructors: `x instanceof Foo` where `Foo` is a plain
    // function used with `new`. `js_new_function_construct` stamps each
    // instance with `synthetic_class_id_for_function(Foo)`; derive the same
    // id from the function value here and walk the candidate's class chain
    // against it. Mirrors the construct site so the common
    // `if (!(this instanceof Foo)) return new Foo()` guard resolves to true
    // inside a `new`-invoked body instead of recursing forever (#838 followup).
    let synthetic_cid = synthetic_class_id_for_function(type_ref);
    if synthetic_cid != 0 {
        let r = js_instanceof(value, synthetic_cid);
        if r.to_bits() == crate::value::TAG_TRUE {
            return r;
        }
        // #5989: the synthetic class-id chain is stamped at construction and does
        // NOT reflect a prototype installed later via
        // `Fn.prototype = Object.create(Base.prototype)` — the classic ES5
        // inheritance idiom react-server-dom's flight Chunk uses to inherit
        // `Promise.prototype` (so `chunk instanceof Promise` wrongly returned
        // false). Fall back to the spec `OrdinaryHasInstance` prototype-chain
        // walk: Perry already walks the real chain for method lookup and
        // `getPrototypeOf`, so only this id-based shortcut missed it.
        if ordinary_has_instance_prototype_walk(value, type_ref) {
            return f64::from_bits(crate::value::TAG_TRUE);
        }
        return f64::from_bits(TAG_FALSE);
    }
    // #2909: nothing recognized the RHS as a constructor/class. Per the
    // ECMAScript `InstanceofOperator`, the right operand must be an object
    // (and ultimately callable / have a `Symbol.hasInstance`); a primitive
    // or non-callable RHS is a `TypeError`, not a silent `false`. Match
    // Node's two distinct messages:
    //   - primitive RHS (number/string/bool/null/undefined/bigint/symbol):
    //       "Right-hand side of 'instanceof' is not an object"
    //   - object-but-non-callable RHS ({}, [], Map, …):
    //       "Right-hand side of 'instanceof' is not callable"
    // (Callable RHS values never reach here — they resolve to a synthetic
    // class id above — so we don't need to model the arrow-`.prototype`
    // case at this site.)
    //
    // #3662: `OrdinaryHasInstance` (the `@@hasInstance` reflective path) wants
    // `false` here, not a `TypeError`; it sets this flag for the call.
    if SUPPRESS_INSTANCEOF_RHS_THROW.with(|c| c.get()) {
        return f64::from_bits(TAG_FALSE);
    }
    throw_invalid_instanceof_rhs(type_ref)
}

/// Spec `OrdinaryHasInstance(C, O)` prototype-chain walk (ECMA-262 7.3.20 steps
/// 4-7): `P = Get(C, "prototype")`; walk `O`'s `[[Prototype]]` chain and return
/// `true` iff some link is identical to `P`. Used as a fallback for the
/// synthetic class-id shortcut, which is stamped at construction and misses a
/// prototype installed via `Fn.prototype = Object.create(Base.prototype)`.
fn ordinary_has_instance_prototype_walk(value: f64, type_ref: f64) -> bool {
    extern "C" {
        fn js_object_get_prototype_of(obj_value: f64) -> f64;
    }
    // P = type_ref.prototype (the constructor's `.prototype` data property).
    let proto = unsafe {
        crate::value::js_dynamic_object_get_property(
            type_ref,
            b"prototype".as_ptr() as *const i8,
            9,
        )
    };
    let target = proto_identity_addr(proto);
    if target == 0 {
        return false; // non-object `.prototype` can never be on the chain
    }
    // Walk `value`'s real [[Prototype]] chain looking for identity with P.
    let mut cur = unsafe { js_object_get_prototype_of(value) };
    let mut depth = 0usize;
    while depth < 100_000 {
        if crate::value::JSValue::from_bits(cur.to_bits()).is_null() {
            return false;
        }
        let cur_addr = proto_identity_addr(cur);
        if cur_addr == 0 {
            return false;
        }
        if cur_addr == target {
            return true;
        }
        cur = unsafe { js_object_get_prototype_of(cur) };
        depth += 1;
    }
    false
}

/// Normalize a value to its heap-pointer address for prototype identity
/// comparison — a NaN-boxed `POINTER_TAG` value or a raw heap pointer both
/// resolve to their address; any non-pointer yields 0.
fn proto_identity_addr(v: f64) -> usize {
    let bits = v.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else if top16 == 0 && bits >= 0x1000 {
        bits as usize
    } else {
        0
    }
}

#[cold]
fn throw_invalid_instanceof_rhs(type_ref: f64) -> ! {
    if rhs_is_object_value(type_ref) {
        throw_type_error(b"Right-hand side of 'instanceof' is not callable");
    }
    throw_type_error(b"Right-hand side of 'instanceof' is not an object");
}

/// `%Function.prototype% [ @@hasInstance ]` (#3662). Spec: return
/// `OrdinaryHasInstance(this, V)`. Unlike the `instanceof` *operator* — which
/// throws a `TypeError` on a non-callable right-hand side — `OrdinaryHasInstance`
/// returns `false` when `this` is not callable, so `Function.prototype[Symbol
/// .hasInstance].call(undefined, {})` is `false` (not a throw). Installed on
/// `Function.prototype` under the `@@hasInstance` key; the receiver flows in
/// through `IMPLICIT_THIS` set by the `.call`/member dispatch.
pub(crate) extern "C" fn function_prototype_has_instance_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let constructor = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let result = ordinary_has_instance(constructor, value);
    f64::from_bits(if result {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

/// `OrdinaryHasInstance(C, O)` without the throwing semantics of the operator:
/// a non-callable `C` (or one whose constructor identity Perry cannot resolve)
/// yields `false` rather than a `TypeError`. Delegates to the operator's full
/// constructor-resolution path (`js_instanceof_dynamic`) with the unresolved-RHS
/// throw suppressed for the duration of the call, so every constructor shape the
/// operator understands (class objects, bound natives like `Array`/`Map`,
/// INT32 class-refs, synthetic function class ids, …) resolves identically.
fn ordinary_has_instance(constructor: f64, value: f64) -> bool {
    let prev = SUPPRESS_INSTANCEOF_RHS_THROW.with(|c| c.replace(true));
    let result = js_instanceof_dynamic(value, constructor);
    SUPPRESS_INSTANCEOF_RHS_THROW.with(|c| c.set(prev));
    result.to_bits() == crate::value::TAG_TRUE
}

thread_local! {
    /// When set, `js_instanceof_dynamic` returns `false` instead of throwing on
    /// an unresolved / non-callable right-hand side. Used by
    /// `OrdinaryHasInstance` (#3662), whose spec returns `false` there rather
    /// than the `TypeError` the `instanceof` operator raises.
    static SUPPRESS_INSTANCEOF_RHS_THROW: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// Whether `value` is a (non-callable) object for the purposes of the
/// `instanceof` RHS check: any heap pointer (plain object, array, Map, Set,
/// Date, RegExp, Buffer/typed-array, etc.). Primitives — including `null`,
/// `undefined`, numbers, strings, booleans, bigints — are not objects.
fn rhs_is_object_value(value: f64) -> bool {
    let bits = value.to_bits();
    let jsval = crate::JSValue::from_bits(bits);
    if jsval.is_null()
        || jsval.is_undefined()
        || jsval.is_bool()
        || jsval.is_any_string()
        || jsval.is_int32()
        || jsval.is_bigint()
    {
        return false;
    }
    if jsval.is_pointer() {
        let ptr = (bits & crate::value::POINTER_MASK) as usize;
        // Symbols are primitives; small registry handles aren't real objects
        // here either, but they're still object-typed in JS (`typeof` is
        // "object"), so a "not callable" message is the right one for them.
        if crate::value::addr_class::is_above_handle_band(ptr)
            && crate::symbol::is_registered_symbol(ptr)
        {
            return false;
        }
        return true;
    }
    // Raw bitcast pointers (typed arrays / buffers / arrays) — these are
    // objects too.
    let top16 = bits >> 48;
    if top16 == 0 && bits >= 0x1000 {
        let addr = bits as usize;
        return crate::buffer::is_registered_buffer(addr)
            || crate::set::is_registered_set(addr)
            || crate::map::is_registered_map(addr)
            || crate::typedarray::lookup_typed_array_kind(addr).is_some()
            || addr >= crate::gc::GC_HEADER_SIZE;
    }
    false
}

#[cold]
fn throw_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Outcome of consulting an own `@@hasInstance` on the `instanceof` RHS.
enum HasInstanceOutcome {
    /// The own `@@hasInstance` was callable; this is the NaN-boxed boolean it
    /// produced (already `ToBoolean`-normalized).
    Result(f64),
    /// No usable own `@@hasInstance` — the property held `undefined`/`null`, so
    /// fall through to the ordinary `instanceof` algorithm.
    Fallthrough,
}

/// Spec `InstanceofOperator` / `GetMethod(C, @@hasInstance)`: a `null`/`undefined`
/// value means "no hook" (→ ordinary `instanceof`), but a present **non-callable**
/// value is a `TypeError` rather than a silent fall-through. `cb` is the already
/// resolved OWN `@@hasInstance` value; this never resolves the inherited
/// `Function.prototype` default thunk, so there is no `instanceof` recursion.
fn dispatch_own_has_instance(cb: f64, value: f64) -> HasInstanceOutcome {
    let jv = crate::JSValue::from_bits(cb.to_bits());
    if jv.is_undefined() || jv.is_null() {
        return HasInstanceOutcome::Fallthrough;
    }
    if !value_is_callable(cb) {
        throw_type_error(b"Symbol(Symbol.hasInstance) is not a function");
    }
    let args = [value];
    let r = unsafe { crate::closure::js_native_call_value(cb, args.as_ptr(), 1) };
    HasInstanceOutcome::Result(if crate::value::js_is_truthy(r) != 0 {
        f64::from_bits(crate::value::TAG_TRUE)
    } else {
        f64::from_bits(crate::value::TAG_FALSE)
    })
}

fn is_event_emitter_instance_value(value: f64) -> bool {
    if let Some(handle) = small_native_handle_id(value) {
        if let Some(probe) = crate::object::event_emitter_handle_probe() {
            return unsafe { probe(handle) };
        }
        return false;
    }

    if crate::node_stream::is_classic_stream_instance_value(value)
        || is_stream_event_emitter_prototype_value(value)
    {
        return true;
    }
    false
}

fn is_event_emitter_async_resource_instance_value(value: f64) -> bool {
    let Some(handle) = small_native_handle_id(value) else {
        return false;
    };
    if let Some(probe) = crate::object::event_emitter_async_resource_handle_probe() {
        return unsafe { probe(handle) };
    }
    false
}

/// `x instanceof <non-constructor built-in>` — the RHS (e.g. `Math`, `JSON`,
/// `Reflect`, `Atomics`) is a namespace object with no `[[Call]]`/`[[Construct]]`,
/// so `InstanceofOperator` throws a `TypeError` ("Right-hand side ... is not
/// callable") regardless of the LHS. The codegen recognizes these statically
/// (they never map to a real class id) and calls this instead of the
/// `js_instanceof(_, 0)` fold, which would wrongly return `false`. Honors the
/// `SUPPRESS_INSTANCEOF_RHS_THROW` scope used by `Symbol.hasInstance` helpers.
#[no_mangle]
pub extern "C" fn js_instanceof_noncallable_rhs() -> f64 {
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if SUPPRESS_INSTANCEOF_RHS_THROW.with(|c| c.get()) {
        return f64::from_bits(TAG_FALSE);
    }
    throw_type_error(b"Right-hand side of 'instanceof' is not callable");
}

/// Check if a value is an instance of a class with the given class_id
/// Walks the inheritance chain to check parent classes
/// Returns NaN-boxed TAG_TRUE / TAG_FALSE so the result identifies as a boolean.
#[no_mangle]
pub extern "C" fn js_instanceof(value: f64, class_id: u32) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let true_val = f64::from_bits(TAG_TRUE);
    let false_val = f64::from_bits(TAG_FALSE);

    if class_id == 0 {
        return false_val;
    }
    // `proxy instanceof C` follows the proxy's prototype chain, which forwards
    // to the target (absent a `getPrototypeOf` trap) — so unwrap to the target
    // before walking the class chain. The proxy is a small id with no chain of
    // its own. (drizzle's aliased-column proxies + `is(value, type)`.)
    let mut value = value;
    {
        let mut depth = 0;
        while depth < 16 && crate::proxy::js_proxy_is_proxy(value) != 0 {
            value = crate::proxy::js_proxy_target(value);
            depth += 1;
        }
    }
    // User-defined `Symbol.hasInstance` takes precedence over the built-in
    // prototype-chain walk — and over the ordinary class-chain fast path below.
    // `new C() instanceof C` must run a class-level `@@hasInstance` rather than
    // short-circuit on the chain (the hook can return `false` for a real
    // instance), so both hook forms are consulted here, ahead of that walk.
    //
    // Form 1: the HIR lifts `static [Symbol.hasInstance](v)` to a top-level
    // function `__perry_wk_hasinstance_<class>` and the LLVM backend registers a
    // pointer to it against the class id at module init.
    if let Some(func_ptr) = lookup_has_instance_hook(class_id) {
        let hook: extern "C" fn(f64) -> f64 = unsafe { std::mem::transmute(func_ptr as *const u8) };
        let result = hook(value);
        // Normalize: any truthy NaN-boxed bool stays as the TAG_TRUE/FALSE
        // sentinel. User-written `return typeof v === "number" && ...`
        // already returns a NaN-boxed bool, so this is usually a no-op.
        let rbits = result.to_bits();
        if rbits == TAG_TRUE || rbits == TAG_FALSE {
            return result;
        }
        // Fallback: treat as truthy → TRUE, zero/undefined → FALSE.
        if result.is_nan() && rbits & 0xFFFF_0000_0000_0000 == 0x7FFC_0000_0000_0000 {
            return false_val;
        }
        if result == 0.0 || result.is_nan() {
            return false_val;
        }
        return true_val;
    }

    // Form 2: the `Object.defineProperty(C, Symbol.hasInstance, { value: fn })`
    // form (zod 4) stores the closure in the class static-symbol table. Read it
    // off the class id (OWN lookup only — never resolves Function.prototype's
    // default @@hasInstance thunk, so no recursion). A present-but-non-callable
    // value throws; only `null`/`undefined` falls through to the chain.
    {
        let hi_sym = crate::symbol::well_known_symbol("hasInstance");
        if !hi_sym.is_null() {
            let hi_f64 = f64::from_bits(crate::value::JSValue::pointer(hi_sym as *const u8).bits());
            if let Some(vb) = crate::symbol::class_static_symbol_lookup(class_id, hi_f64) {
                let cb = f64::from_bits(vb);
                if let HasInstanceOutcome::Result(r) = dispatch_own_has_instance(cb, value) {
                    return r;
                }
            }
        }
    }

    // Subclass-of-built-in: `class S extends Array {}` produces a real
    // ObjectHeader instance whose class-id chain reaches the built-in's
    // reserved class id (a parent edge registered at module init). The
    // per-built-in probes below short-circuit to `false` for such an
    // instance (it isn't a *real* Array/Map/Error/…), so walk the object's
    // own class chain up front. Only genuine `GC_TYPE_OBJECT` instances carry
    // a `class_id` field — real Arrays/Maps/Errors have other GC types and
    // fall through to their dedicated probes unchanged. Refs
    // class/subclass-builtins/* and class/subclass/builtin-objects/*.
    {
        let jv = crate::JSValue::from_bits(value.to_bits());
        if jv.is_pointer() {
            let obj = jv.as_pointer::<ObjectHeader>();
            if crate::value::addr_class::is_above_handle_band(obj as usize) {
                let gc_header = unsafe {
                    (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader
                };
                if unsafe { (*gc_header).obj_type } == crate::gc::GC_TYPE_OBJECT {
                    let mut cur = unsafe { (*obj).class_id };
                    if cur != 0 {
                        if cur == class_id {
                            return true_val;
                        }
                        let mut depth = 0;
                        while let Some(pid) = get_parent_class_id(cur) {
                            if pid == 0 || depth > 64 {
                                break;
                            }
                            if pid == class_id {
                                return true_val;
                            }
                            cur = pid;
                            depth += 1;
                        }
                    }
                }
            }
        }
    }
    // Temporal reference types (`d instanceof Temporal.Duration`, …). A Temporal
    // value is a NaN-boxed pointer to a brand-tagged cell, not an ObjectHeader
    // with a class chain, so probe the cell's brand kind directly. Keep the band
    // in sync with perry-runtime/src/temporal/mod.rs.
    if (crate::temporal::CLASS_ID_TEMPORAL_FIRST..=crate::temporal::CLASS_ID_TEMPORAL_LAST)
        .contains(&class_id)
    {
        return if crate::temporal::temporal_value_matches_class_id(value, class_id) {
            true_val
        } else {
            false_val
        };
    }
    // `value instanceof Function` — true for any callable value. Per
    // `OrdinaryHasInstance`, every Perry function (declaration, expression,
    // arrow, method, bound function, native handle, built-in constructor)
    // has `Function.prototype` in its prototype chain. Keep `CLASS_ID_FUNCTION`
    // in sync with perry-codegen/src/expr/instance_misc1.rs.
    if class_id == CLASS_ID_FUNCTION {
        return if value_is_callable(value) {
            true_val
        } else {
            false_val
        };
    }
    // Keep in sync with perry-codegen/src/expr/instance_misc1.rs.
    let classic_stream_name = match class_id {
        0xFFFF0070 => Some("Stream"),
        0xFFFF0071 => Some("Readable"),
        0xFFFF0072 => Some("Writable"),
        0xFFFF0073 => Some("Duplex"),
        0xFFFF0074 => Some("Transform"),
        0xFFFF0075 => Some("PassThrough"),
        _ => None,
    };
    if let Some(name) = classic_stream_name {
        return if crate::node_stream::is_classic_stream_instance_of(value, name) {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_EVENT_EMITTER {
        return if is_event_emitter_instance_value(value) {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_EVENT_EMITTER_ASYNC_RESOURCE {
        return if is_event_emitter_async_resource_instance_value(value) {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_NET_SOCKET {
        return if let (Some(handle), Some(probe)) = (
            small_native_handle_id(value),
            crate::object::net_socket_handle_probe(),
        ) {
            if unsafe { probe(handle) } {
                true_val
            } else {
                false_val
            }
        } else {
            false_val
        };
    }
    if class_id == crate::fs::CLASS_ID_FS_STATS_EXPORT {
        return if crate::fs::is_fs_stats_instance_value(value) {
            true_val
        } else {
            false_val
        };
    }
    if class_id == crate::fs::CLASS_ID_FS_DIR {
        return if crate::fs::is_fs_dir_instance_value(value) {
            true_val
        } else {
            false_val
        };
    }
    if class_id == crate::fs::CLASS_ID_FS_DIRENT {
        return if crate::fs::is_fs_dirent_instance_value(value) {
            true_val
        } else {
            false_val
        };
    }
    if class_id == crate::fs::CLASS_ID_FS_READ_STREAM {
        return if crate::fs::is_fs_stream_instance_value(value, "ReadStream") {
            true_val
        } else {
            false_val
        };
    }
    if class_id == crate::fs::CLASS_ID_FS_WRITE_STREAM {
        return if crate::fs::is_fs_stream_instance_value(value, "WriteStream") {
            true_val
        } else {
            false_val
        };
    }
    if class_id == crate::fs::CLASS_ID_FS_UTF8_STREAM {
        return if crate::fs::is_fs_stream_instance_value(value, "Utf8Stream") {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_CRYPTO {
        return if is_native_module_namespace_value(value, "crypto.webcrypto") {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_SUBTLE_CRYPTO {
        return if is_native_module_namespace_value(value, "crypto.subtle") {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_CRYPTO_KEY {
        let addr = value_addr(value);
        return if addr != 0 && crate::buffer::crypto_key_meta(addr).is_some() {
            true_val
        } else {
            false_val
        };
    }

    let bits = value.to_bits();
    let jsval = crate::JSValue::from_bits(bits);

    // Special handling for Uint8Array/Buffer (class_id 0xFFFF0004)
    // Perry buffers are raw BufferHeader pointers bitcast to f64 (not NaN-boxed),
    // so the normal POINTER_TAG check doesn't work for them.
    // We use a thread-local buffer registry to identify buffer pointers.
    if class_id == crate::buffer::BUFFER_TYPE_ID {
        // Check if NaN-boxed pointer
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::buffer::is_registered_buffer(addr) {
                return true_val;
            }
        }
        // Check if raw pointer (buffer values are bitcast, not NaN-boxed)
        let top16 = (bits >> 48) as u16;
        if top16 == 0 && bits >= 0x1000 && crate::buffer::is_registered_buffer(bits as usize) {
            return true_val;
        }
        return false_val;
    }

    // ArrayBuffer — Perry models ArrayBuffer storage with BufferHeader values
    // marked in a side registry. They can arrive either NaN-boxed or as raw
    // buffer pointers, matching the Buffer/Uint8Array path above.
    const CLASS_ID_ARRAY_BUFFER: u32 = 0xFFFF0025;
    if class_id == CLASS_ID_ARRAY_BUFFER {
        let addr = if jsval.is_pointer() {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            let top16 = (bits >> 48) as u16;
            if top16 == 0 && bits >= 0x1000 {
                bits as usize
            } else {
                0
            }
        };
        if addr != 0
            && crate::buffer::is_registered_buffer(addr)
            && crate::buffer::is_array_buffer(addr)
        {
            return true_val;
        }
        return false_val;
    }

    // #1545: Web Streams `instanceof ReadableStream` / `instanceof
    // WritableStream`. Stream handles are numeric `id as f64`, so consult the
    // stdlib kind-probe (1 = readable, 2 = writable) rather than the class
    // chain. Covers `ts.readable instanceof ReadableStream`,
    // `rs.pipeThrough(ts) instanceof ReadableStream`, etc.
    // kind probe values: 1 = readable, 2 = writable, 5 = transform
    // (3 = reader, 4 = writer — not user-facing instanceof targets here).
    const CLASS_ID_READABLE_STREAM: u32 = 0xFFFF0060;
    const CLASS_ID_WRITABLE_STREAM: u32 = 0xFFFF0061;
    const CLASS_ID_TRANSFORM_STREAM: u32 = 0xFFFF0062;
    if class_id == CLASS_ID_READABLE_STREAM
        || class_id == CLASS_ID_WRITABLE_STREAM
        || class_id == CLASS_ID_TRANSFORM_STREAM
    {
        if value.is_finite() && value > 0.0 && value.fract() == 0.0 {
            if let Some(probe) = crate::object::stream_handle_kind_probe() {
                let kind = unsafe { probe(value as usize) };
                let want = match class_id {
                    CLASS_ID_READABLE_STREAM => 1,
                    CLASS_ID_WRITABLE_STREAM => 2,
                    _ => 5, // CLASS_ID_TRANSFORM_STREAM
                };
                if kind == want {
                    return true_val;
                }
            }
        }
        return false_val;
    }

    // WHATWG fetch: `instanceof Response` / `Request` / `Headers` / `Blob`.
    // These are pointer-tagged small-integer handles (stdlib fetch registries),
    // not heap objects, so consult the stdlib fetch kind-probe rather than the
    // class chain. Without this, Hono's `res instanceof Response` route-fallback
    // guard sees `false` and skips the fallback, escaping a bare sentinel.
    const CLASS_ID_RESPONSE: u32 = 0xFFFF0028;
    const CLASS_ID_REQUEST: u32 = 0xFFFF0029;
    const CLASS_ID_HEADERS: u32 = 0xFFFF002A;
    const CLASS_ID_BLOB: u32 = 0xFFFF0026;
    if class_id == CLASS_ID_RESPONSE
        || class_id == CLASS_ID_REQUEST
        || class_id == CLASS_ID_HEADERS
        || class_id == CLASS_ID_BLOB
    {
        let want = match class_id {
            CLASS_ID_RESPONSE => 1u8,
            CLASS_ID_REQUEST => 2,
            CLASS_ID_HEADERS => 3,
            _ => 4, // CLASS_ID_BLOB
        };
        if let Some(handle) = small_native_handle_id(value) {
            if let Some(probe) = crate::object::fetch_handle_kind_probe() {
                if unsafe { probe(handle as usize) } == want {
                    return true_val;
                }
            }
        }
        // `class X extends Request/Response` instance: a heap object that
        // stashes the underlying native fetch handle id under
        // `__perry_fetch_handle__`. Unwrap and probe so `sub instanceof
        // Request` is true, matching a bare handle.
        if jsval.is_pointer() {
            let raw = jsval.as_pointer::<u8>() as usize;
            if let Some(id) = unsafe { crate::object::fetch_subclass_handle_id(raw) } {
                if let Some(probe) = crate::object::fetch_handle_kind_probe() {
                    if unsafe { probe(id as usize) } == want {
                        return true_val;
                    }
                }
            }
        }
        // A Blob can also be a real heap object allocated with CLASS_ID_BLOB
        // (e.g. `stream/consumers`.`blob()` and `blob_value_from_bytes`), not
        // just a small fetch-registry handle. Match it by its own class id so
        // `blob instanceof Blob` is true for that representation too.
        if class_id == CLASS_ID_BLOB && jsval.is_pointer() {
            let obj = jsval.as_pointer::<ObjectHeader>();
            if crate::value::addr_class::is_above_handle_band(obj as usize)
                && unsafe { (*obj).class_id } == CLASS_ID_BLOB
            {
                return true_val;
            }
        }
        return false_val;
    }

    // Built-in JS types Map / Set / RegExp / Date — Perry doesn't define
    // user classes for these, so we use reserved class IDs and detect via
    // the per-type registries (MAP_REGISTRY / SET_REGISTRY / REGEX_POINTERS)
    // or, for Date, by checking that the value is a finite f64 timestamp.
    const CLASS_ID_DATE: u32 = 0xFFFF0020;
    const CLASS_ID_REGEXP: u32 = 0xFFFF0021;
    const CLASS_ID_MAP: u32 = 0xFFFF0022;
    const CLASS_ID_SET: u32 = 0xFFFF0023;
    if class_id == CLASS_ID_DATE {
        // A Perry Date is a NaN-boxed pointer to a `DateCell` (#2089). Its
        // identity is the cell's `GcHeader` type, so `new Date(NaN)` (an
        // Invalid Date — a cell whose time value is NaN) matches just like
        // any other Date, and a plain number never matches.
        if crate::date::is_date_value(value) {
            return true_val;
        }
        return false_val;
    }
    if class_id == CLASS_ID_MAP {
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::map::is_registered_map(addr) {
                return true_val;
            }
        }
        return false_val;
    }
    if class_id == CLASS_ID_SET {
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::set::is_registered_set(addr) {
                return true_val;
            }
        }
        return false_val;
    }
    // #5834: `x instanceof WeakMap`/`WeakSet` for a REAL instance. These
    // reserved ids (kept in sync with perry-codegen/src/expr/instance_misc1.rs)
    // are distinct from the runtime `CLASS_ID_WEAKMAP`/`CLASS_ID_WEAKSET`
    // stamped on actual instances (weakref.rs) — the subclass-chain walk above
    // only matches a `class S extends WeakMap {}` instance (whose chain reaches
    // this reserved id), so a genuine `new WeakMap()` still needs its own probe
    // here, same shape as Map/Set above.
    const CLASS_ID_WEAKMAP_RESERVED: u32 = 0xFFFF002C;
    const CLASS_ID_WEAKSET_RESERVED: u32 = 0xFFFF002D;
    if class_id == CLASS_ID_WEAKMAP_RESERVED {
        return if crate::weakref::weak_class_id_from_receiver(value)
            == Some(crate::weakref::CLASS_ID_WEAKMAP)
        {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_WEAKSET_RESERVED {
        return if crate::weakref::weak_class_id_from_receiver(value)
            == Some(crate::weakref::CLASS_ID_WEAKSET)
        {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_REGEXP {
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::regex::is_regex_pointer(addr as *const u8) {
                return true_val;
            }
        }
        return false_val;
    }
    if class_id == CLASS_ID_PROMISE {
        return if crate::promise::js_value_is_promise(value) != 0 {
            true_val
        } else {
            false_val
        };
    }

    // `Object` — ECMAScript spec: `x instanceof Object` is true for any
    // non-primitive (every object/array/function/Map/Set/Buffer/RegExp/
    // Date/typed-array/Promise/etc.). The codegen maps `Object` to this
    // reserved id (#585 follow-up: pre-#585 fix this case worked by
    // accident because the codegen produced `class_id = 0` and the
    // runtime returned true via `0 == 0` on the obj_class_id check).
    const CLASS_ID_OBJECT: u32 = 0xFFFF0050;
    if class_id == CLASS_ID_OBJECT {
        if jsval.is_pointer() {
            // Covers every heap object, including a Date (now a NaN-boxed
            // `DateCell` pointer — #2089) and an Invalid Date.
            return true_val;
        }
        let top16 = (bits >> 48) as u16;
        if top16 == 0 && bits >= 0x1000 {
            let addr = bits as usize;
            if crate::buffer::is_registered_buffer(addr)
                || crate::set::is_registered_set(addr)
                || crate::map::is_registered_map(addr)
                || crate::typedarray::lookup_typed_array_kind(addr).is_some()
            {
                return true_val;
            }
        }
        return false_val;
    }

    // Array — Perry arrays are heap allocations with `GC_TYPE_ARRAY` in
    // their gc_header (one byte at obj-8). Pointer can arrive NaN-boxed
    // (POINTER_TAG) or as a raw bitcast f64; handle both. Lazy arrays
    // (Phase 5 JSON.parse result) are also arrays from the user's
    // perspective — must return true without force-materializing.
    const CLASS_ID_ARRAY: u32 = 0xFFFF0024;
    if class_id == CLASS_ID_ARRAY {
        let addr = if jsval.is_pointer() {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            let top16 = (bits >> 48) as u16;
            if top16 == 0 && bits >= 0x1000 {
                bits as usize
            } else {
                0
            }
        };
        if addr != 0 && addr >= crate::gc::GC_HEADER_SIZE {
            let gc_header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            unsafe {
                let obj_type = (*gc_header).obj_type;
                if obj_type == crate::gc::GC_TYPE_ARRAY || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
                {
                    return true_val;
                }
            }
        }
        return false_val;
    }

    // Typed arrays — Int8Array..Float16Array reserved IDs (0xFFFF0030..3B).
    // The pointer can arrive as either a NaN-boxed POINTER_TAG value or a
    // raw bitcast f64, so handle both forms.
    if (0xFFFF0030..=0xFFFF003B).contains(&class_id) {
        let addr = if jsval.is_pointer() {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            let top16 = (bits >> 48) as u16;
            if top16 == 0 && bits >= 0x1000 {
                bits as usize
            } else {
                0
            }
        };
        if addr != 0 {
            if let Some(actual_kind) = crate::typedarray::lookup_typed_array_kind(addr) {
                let want_id = crate::typedarray::class_id_for_kind(actual_kind);
                if want_id == class_id {
                    return true_val;
                }
            }
        }
        return false_val;
    }

    // Only objects (pointers) can be instances of classes
    if !jsval.is_pointer() {
        return false_val;
    }

    // Get the object pointer
    let obj_ptr = jsval.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return false_val;
    }

    // Refs #421: NaN-boxed POINTER_TAG values whose unboxed payload is a
    // small registry id (Web Fetch handles, sockets, DB connections, etc.)
    // are NOT real ObjectHeader pointers — reading the GC header at
    // `obj_ptr - 8` would SIGSEGV on unmapped memory. They aren't instances
    // of any user-defined class either, so return false unconditionally.
    if crate::value::addr_class::is_handle_band(obj_ptr as usize) {
        return false_val;
    }

    unsafe {
        // Special handling for built-in Error and its subclasses (TypeError, RangeError, etc.).
        // ErrorHeader uses GC_TYPE_ERROR; we match by error_kind against the requested CLASS_ID_*.
        let gc_header =
            (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type == crate::gc::GC_TYPE_ERROR {
            let err_ptr = obj_ptr as *const crate::error::ErrorHeader;
            let kind = (*err_ptr).error_kind;
            if class_id == crate::event_target::CLASS_ID_DOM_EXCEPTION {
                return if crate::event_target::is_dom_exception_error(err_ptr) {
                    true_val
                } else {
                    false_val
                };
            }
            return match class_id {
                crate::error::CLASS_ID_ERROR => true_val,
                crate::error::CLASS_ID_TYPE_ERROR => {
                    if kind == crate::error::ERROR_KIND_TYPE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_RANGE_ERROR => {
                    if kind == crate::error::ERROR_KIND_RANGE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_REFERENCE_ERROR => {
                    if kind == crate::error::ERROR_KIND_REFERENCE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_SYNTAX_ERROR => {
                    if kind == crate::error::ERROR_KIND_SYNTAX_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_EVAL_ERROR => {
                    if kind == crate::error::ERROR_KIND_EVAL_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_URI_ERROR => {
                    if kind == crate::error::ERROR_KIND_URI_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_AGGREGATE_ERROR => {
                    if kind == crate::error::ERROR_KIND_AGGREGATE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                _ => false_val,
            };
        }

        if gc_type == crate::gc::GC_TYPE_OBJECT {
            if let Some(matches) =
                crate::perf_hooks::is_perf_hooks_shape_instance_of(value, class_id)
            {
                return if matches { true_val } else { false_val };
            }
            if let Some(matches) =
                crate::perf_hooks::is_perf_entry_object_instance_of(obj_ptr, class_id)
            {
                return if matches { true_val } else { false_val };
            }
        }

        // For user-defined classes that extend Error: `myErr instanceof Error` should be true.
        if class_id == crate::error::CLASS_ID_ERROR {
            let obj_class_id = (*obj_ptr).class_id;
            if extends_builtin_error(obj_class_id) {
                return true_val;
            }
        }

        // Check if the object's class_id matches directly
        let obj_class_id = (*obj_ptr).class_id;
        if class_id == crate::event_target::CLASS_ID_EVENT
            && obj_class_id == crate::event_target::CLASS_ID_CUSTOM_EVENT
        {
            return true_val;
        }
        if obj_class_id == class_id {
            return true_val;
        }

        // Walk up the inheritance chain using the class registry
        let mut current_class = obj_class_id;
        while let Some(parent_id) = get_parent_class_id(current_class) {
            if parent_id == 0 {
                break;
            }
            if parent_id == class_id {
                return true_val;
            }
            current_class = parent_id;
        }

        false_val
    }
}
