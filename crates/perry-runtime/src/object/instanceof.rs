//! `instanceof` evaluation: `js_instanceof` and the dynamic
//! (runtime-class-ref) form `js_instanceof_dynamic`.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation.

use super::*;

// Keep in sync with perry-codegen/src/expr/instance_misc1.rs.
const CLASS_ID_EVENT_EMITTER: u32 = 0xFFFF0076;
const CLASS_ID_EVENT_EMITTER_ASYNC_RESOURCE: u32 = 0xFFFF0077;
const CLASS_ID_ASYNC_LOCAL_STORAGE: u32 = 0xFFFF0078;
const CLASS_ID_ASYNC_RESOURCE: u32 = 0xFFFF0079;
const CLASS_ID_PROMISE: u32 = 0xFFFF0027;
const CLASS_ID_NET_SOCKET: u32 = 0xFFFF00B4;
const CLASS_ID_CRYPTO: u32 = 0xFFFF00C0;
const CLASS_ID_SUBTLE_CRYPTO: u32 = 0xFFFF00C1;
const CLASS_ID_CRYPTO_KEY: u32 = 0xFFFF00C2;
/// `value instanceof Function` reserved id (see `js_instanceof`).
const CLASS_ID_FUNCTION: u32 = 0xFFFF00F0;

mod dynamic_dispatch;
mod static_dispatch;

pub use dynamic_dispatch::js_instanceof_dynamic;
pub use static_dispatch::js_instanceof;

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
    // `class_ref_id` also requires `is_class_id_registered`, so a
    // user-crafted NaN payload sharing this tag band (e.g. via
    // `DataView.setFloat64` — a real JS number, not a class ref) is not
    // misclassified as callable.
    if class_ref_id(value).is_some() {
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

/// Candidate heap address of an `instanceof` operand; 0 for every primitive.
/// #10479: this used to treat every tag band `>= 0x7FF8` as a pointer, so a
/// 1-5 byte inline string (or an INT32 class ref) reached
/// `object_static_prototype` as a garbage address and segfaulted.
#[inline]
fn value_addr(value: f64) -> usize {
    crate::value::addr_class::object_ref_addr(value)
}

fn recorded_prototype_instanceof_builtin(value: f64, name: &str) -> Option<bool> {
    let addr = value_addr(value);
    if addr == 0 || super::prototype_chain::object_static_prototype(addr).is_none() {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let constructor = scope.root_nanbox_f64(crate::object::js_get_global_this_builtin_value(
        name.as_ptr(),
        name.len(),
    ));
    if !value_is_callable(constructor.get_nanbox_f64()) {
        return None;
    }
    Some(ordinary_has_instance_prototype_walk(
        value.get_nanbox_f64(),
        constructor.get_nanbox_f64(),
    ))
}

fn is_native_module_namespace_value(value: f64, expected: &str) -> bool {
    let jv = crate::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let obj = jv.as_pointer::<ObjectHeader>();
    // #10556: a native `new EventEmitter()` is a POINTER_TAG registry handle
    // (`0x38000`), and `x instanceof EventEmitter` asks this probe first — the
    // null check alone let it read `class_id` out of unmapped low memory.
    let is_object = unsafe { crate::value::addr_class::try_read_gc_header(obj as usize) }
        .is_some_and(|header| header.obj_type == crate::gc::GC_TYPE_OBJECT);
    if !is_object {
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
/// Map a builtin constructor VALUE (the `ClosureHeader`-backed function installed
/// on `globalThis`) back to the class id that codegen passes to `js_instanceof`
/// for the static `x instanceof <Identifier>` form.
///
/// Only the natively-backed builtins need this: their instances are handles
/// (stream / fetch registries), not heap objects with a real prototype chain, so
/// `js_instanceof` brand-checks them via the kind probes rather than a chain walk.
/// Heap-backed builtins already resolve through the class-id / prototype paths.
fn builtin_ctor_class_id_from_value(type_ref: f64) -> Option<u32> {
    let closure =
        crate::value::js_nanbox_get_pointer(type_ref) as *const crate::closure::ClosureHeader;
    if closure.is_null() {
        return None;
    }
    if unsafe { (*closure).type_tag } != crate::closure::CLOSURE_MAGIC {
        return None;
    }
    let name_value = crate::closure::closure_get_dynamic_prop(closure as usize, "name");
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, len) = crate::string::str_bytes_from_jsvalue(name_value, &mut scratch)?;
    if ptr.is_null() {
        return None;
    }
    let name =
        std::str::from_utf8(unsafe { std::slice::from_raw_parts(ptr, len as usize) }).ok()?;
    let class_id = match name {
        "ReadableStream" => 0xFFFF_0060,
        "WritableStream" => 0xFFFF_0061,
        "TransformStream" => 0xFFFF_0062,
        "Response" => 0xFFFF_0028,
        "Request" => 0xFFFF_0029,
        "Headers" => 0xFFFF_002A,
        "Blob" => 0xFFFF_0026,
        "File" => 0xFFFF_002F,
        _ => return None,
    };
    // The name alone is forgeable (`function Response() {}` in user code).
    // Require IDENTITY with the builtin constructor installed on
    // `globalThis`: only the genuine builtin value brand-checks instances.
    let global_ctor = crate::object::js_get_global_this_builtin_value(name.as_ptr(), name.len());
    if crate::value::js_nanbox_get_pointer(global_ctor) as usize != closure as usize {
        return None;
    }
    Some(class_id)
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
        "SharedArrayBuffer" => 0xFFFF002E,
        "DataView" => 0xFFFF002B,
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
        // #6301: the `X → EventTarget` edge is what makes a user
        // `class Bus extends EventTarget {}` instance resolve the inherited
        // `addEventListener`/`removeEventListener`/`dispatchEvent` surface
        // (see `event_target::class_chain_is_event_target`). Without an id
        // here `js_register_class_parent_dynamic` left the subclass
        // parentless and it inherited nothing.
        "EventTarget" => crate::event_target::CLASS_ID_EVENT_TARGET,
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
    // OrdinaryHasInstance(C, O) step 3 (ECMA-262 7.3.21): "If Type(O) is not
    // Object, return false." A primitive left operand is never `instanceof`
    // anything, so guard it here BEFORE the `js_object_get_prototype_of(value)`
    // walk below. Routing a primitive into that walk is wrong two ways:
    //   * `null`/`undefined` THROW `TypeError: Cannot convert undefined or null
    //     to object` (#6587: find-my-way@9's `FindMyWay(opts)` is called without
    //     `new`, so its `Router` body evaluates `this instanceof Router` with
    //     `this === undefined` — a function-value RHS routes through
    //     `js_instanceof_dynamic`'s synthetic-class-id tail into this walk, and
    //     the unguarded getPrototypeOf aborted module init before any route was
    //     registered);
    //   * a heap-allocated string/bigint/symbol gets ToObject-wrapped by
    //     getPrototypeOf, so the walk climbs the wrapper chain and can spuriously
    //     match (`Symbol() instanceof Object` wrongly returned `true`).
    // Every tag below is checked without dereferencing. Real f64 numbers share
    // tag-space with legacy raw heap pointers (a bare `is_number()` would
    // misclassify a raw-bitcast object), so a number is only rejected when it
    // does not decode as an object address. They are NOT all answered by
    // earlier fast paths: a dynamic `1.5 instanceof Number` / `instanceof
    // Object` reached this walk, ToObject-wrapped the number and matched.
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let type_ref = scope.root_nanbox_f64(type_ref);
    {
        let jv = crate::value::JSValue::from_bits(value.get_nanbox_f64().to_bits());
        if jv.is_null()
            || jv.is_undefined()
            || jv.is_bool()
            || jv.is_int32()
            || jv.is_any_string()
            || jv.is_bigint()
            || (jv.is_number() && value_addr(value.get_nanbox_f64()) == 0)
            || unsafe { crate::symbol::js_is_symbol(value.get_nanbox_f64()) != 0 }
        {
            return false;
        }
    }
    // P = type_ref.prototype (the constructor's `.prototype` data property).
    let proto = unsafe {
        crate::value::js_dynamic_object_get_property(
            type_ref.get_nanbox_f64(),
            b"prototype".as_ptr() as *const i8,
            9,
        )
    };
    let target = proto_identity_addr(proto);
    if target == 0 {
        return false; // non-object `.prototype` can never be on the chain
    }
    // Walk `value`'s real [[Prototype]] chain looking for identity with P.
    let mut cur = unsafe { js_object_get_prototype_of(value.get_nanbox_f64()) };
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

crate::perry_thread_local! {
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
    if is_native_module_namespace_value(value, "cluster.default")
        || crate::cluster::is_worker_instance_value(value)
    {
        return true;
    }
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
    let constructor = crate::object::bound_native_callable_export_value("events", "EventEmitter");
    ordinary_has_instance_prototype_walk(value, constructor)
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

/// Does the class-id chain starting at `start` reach `want`?
///
/// Walks two edges at every step, both bounded by the same depth budget:
///
/// * the **parent** edge (`CLASS_REGISTRY`), i.e. `extends`; and
/// * the **generic-origin** edge (#7575), i.e. "this class is
///   `monomorph`'s `Gen$num`, written by the user as `Gen`".
///
/// The second one exists because Perry monomorphizes generic classes. `new
/// Gen<number>()` stamps the instance with `Gen$num`'s id, while `x instanceof
/// Gen` resolves the RHS to the GENERIC's id — an id that is in no parent chain,
/// so the walk answered `false` for the class the user actually wrote. It is
/// checked at each hop rather than only at the start because a specialization's
/// ancestors can be specializations too.
///
/// The origin edge is NOT a parent edge: `CLASS_REGISTRY`'s chain also resolves
/// `super()` construction, static-method lookup and vtable dispatch, and
/// splicing `Gen` in between `Gen$num` and its real base would re-run the wrong
/// constructor. See `object/class_meta_registry.rs`.
pub(crate) fn class_chain_reaches(start: u32, want: u32) -> bool {
    if start == 0 || want == 0 {
        return false;
    }
    let mut cur = start;
    let mut depth = 0;
    loop {
        if cur == want {
            return true;
        }
        if depth > 64 {
            return false;
        }
        if let Some(gid) = crate::object::class_generic_origin(cur) {
            if gid == want || class_chain_reaches_parents_only(gid, want, depth + 1) {
                return true;
            }
        }
        match get_parent_class_id(cur) {
            Some(pid) if pid != 0 && pid != cur => cur = pid,
            _ => return false,
        }
        depth += 1;
    }
}

/// The parent-only half of [`class_chain_reaches`], used to continue a walk that
/// has already stepped onto a generic origin. Separate so the two edges cannot
/// recurse into each other without bound.
fn class_chain_reaches_parents_only(start: u32, want: u32, depth0: usize) -> bool {
    let mut cur = start;
    let mut depth = depth0;
    while depth <= 64 {
        if cur == want {
            return true;
        }
        match get_parent_class_id(cur) {
            Some(pid) if pid != 0 && pid != cur => cur = pid,
            _ => return false,
        }
        depth += 1;
    }
    false
}

/// #10624: `subclass_of_builtin_reaches`'s armed-latch arm.
fn class_chain_reaches_dynamic_armed(cur: u32, obj: *const ObjectHeader, want: u32) -> bool {
    let pin = super::class_registry::instance_pinned_constructing_class(obj);
    class_chain_reaches_dynamic(cur, pin, want)
}

/// Does the ancestry chain from `start_cid` reach `want`, walking by VALUE
/// while precision is available? `class_chain_reaches` walks purely by
/// class_id through the shared, last-write-wins `CLASS_REGISTRY` —
/// ambiguous once the SAME `ClassExprFresh` template has been evaluated more
/// than once. Each hop here instead prefers, in order: (1) `start_pin`/a
/// pinned VALUE on the current node (`class_object_pinned_parent`, the same
/// per-evaluation edge `super()`/captures already consult), (2)
/// `template_dynamic_parent_value`, the actual parent VALUE for any class_id
/// registered dynamically. Exhausting both degrades to exactly
/// `class_chain_reaches`'s answer — so an instance from an EARLIER
/// evaluation stays correct even after a LATER one overwrote the table.
fn class_chain_reaches_dynamic(start_cid: u32, start_pin: Option<f64>, want: u32) -> bool {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    if start_cid == 0 || want == 0 {
        return false;
    }
    let mut cur = start_cid;
    let mut cur_value = start_pin;
    let mut depth = 0usize;
    loop {
        if cur == want {
            return true;
        }
        if depth > 64 {
            return false;
        }
        if let Some(gid) = crate::object::class_generic_origin(cur) {
            if gid == want || class_chain_reaches_parents_only(gid, want, depth + 1) {
                return true;
            }
        }
        let pinned = cur_value
            .filter(|v| is_class_object_value(*v))
            .and_then(|v| {
                class_object_pinned_parent(
                    crate::value::js_nanbox_get_pointer(v) as *const ObjectHeader
                )
            });
        let next_value = pinned.unwrap_or_else(|| {
            super::class_registry::parent_static::template_dynamic_parent_value(cur)
        });
        if next_value.to_bits() == TAG_UNDEFINED {
            return false;
        }
        let next_cid = dynamic_value_class_id(next_value);
        if next_cid == 0 || next_cid == cur {
            return false;
        }
        cur = next_cid;
        cur_value = Some(next_value);
        depth += 1;
    }
}

/// `class S extends Array {}` produces a real `ObjectHeader` instance whose
/// class-id chain reaches the built-in's reserved class id (a parent edge
/// registered at module init). The per-built-in probes in `js_instanceof`
/// short-circuit to `false` for such an instance (it isn't a *real*
/// Array/Map/Error/…), so walk the object's own class chain up front. Only
/// genuine `GC_TYPE_OBJECT` instances carry a `class_id` field. Refs
/// class/subclass-builtins/* and class/subclass/builtin-objects/*.
///
/// Split out of `js_instanceof` (#10624) so that function's own size, and
/// thus how well its unrelated, far more common paths optimize, does not
/// depend on this ladder's own latch-gated logic.
fn subclass_of_builtin_reaches(value: f64, class_id: u32) -> bool {
    let jv = crate::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let obj = jv.as_pointer::<ObjectHeader>();
    if !crate::value::addr_class::is_above_handle_band(obj as usize) {
        return false;
    }
    let gc_header =
        unsafe { (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader };
    if unsafe { (*gc_header).obj_type } != crate::gc::GC_TYPE_OBJECT {
        return false;
    }
    let cur = unsafe { (*obj).class_id };
    // #10624: only pay for the value-aware walk once something has pinned
    // per-evaluation heritage.
    let reaches =
        if super::class_registry::evaluation_heritage::CLASS_OBJECT_HERITAGE_PIN_LATCH.is_idle() {
            class_chain_reaches(cur, class_id)
        } else {
            class_chain_reaches_dynamic_armed(cur, obj, class_id)
        };
    if reaches {
        return true;
    }

    // #9362: util.inherits(DerivedClass, BaseClass) links DerivedClass.prototype
    // to BaseClass.prototype at runtime; it does not (and must not) create an
    // extends edge between the constructor objects. The class-id fast path
    // above therefore misses even though the observable prototype chain
    // contains BaseClass.prototype. Only pay for the spec prototype walk when
    // the candidate class's declaration prototype has a user-selected parent.
    // The two `class_decl_prototype_object` probes are class registry reads
    // (TLS + RwLock + map, ~130 instructions each) and they ran EAGERLY on
    // every call that got this far — which is every MISS, the path this whole
    // ladder exists to answer `false` on. They exist only to ask a question
    // whose answer is `false` for every receiver in a process that never
    // re-points an object's prototype, and the latch answers that for the
    // whole process in one load. Set, never cleared, and published before the
    // flag it guards, so it can only ever be conservatively true.
    if super::prototype_chain::any_user_prototype_override() {
        let candidate_proto = super::class_registry::class_decl_prototype_object(cur);
        let target_proto = super::class_registry::class_decl_prototype_object(class_id);
        if !candidate_proto.is_null()
            && !target_proto.is_null()
            && super::prototype_chain::object_has_user_prototype_override(candidate_proto as usize)
            && ordinary_has_instance_prototype_walk(
                value,
                super::class_constructor_ref_value(class_id),
            )
        {
            return true;
        }
    }
    false
}

/// Check if a value is an instance of a class with the given class_id
/// Walks the inheritance chain to check parent classes
/// Returns NaN-boxed TAG_TRUE / TAG_FALSE so the result identifies as a boolean.

#[cfg(test)]
mod null_lhs_tests {
    use super::*;

    /// `OrdinaryHasInstance` with a primitive left operand must answer `false`,
    /// never throw or spuriously match. The guard is purely tag-based and
    /// short-circuits before any prototype access, so the RHS is irrelevant and
    /// no arena/runtime state is touched.
    ///
    /// * `null`/`undefined` — #6587: previously threw `Cannot convert undefined
    ///   or null to object` (find-my-way@9's `FindMyWay(opts)` called without
    ///   `new` evaluates `this instanceof Router` with `this === undefined`).
    /// * boolean / int32 / string / bigint — a primitive is never `instanceof`
    ///   anything; a string's ToObject wrapper chain must not spuriously match.
    ///
    /// (Symbols are also guarded, but constructing one needs the runtime arena,
    /// so that arm is covered by the behavioral e2e/compiled tests instead.)
    #[test]
    fn ordinary_has_instance_walk_is_false_for_primitive_lhs() {
        let cases = [
            f64::from_bits(crate::value::TAG_UNDEFINED),
            f64::from_bits(crate::value::TAG_NULL),
            f64::from_bits(crate::value::TAG_TRUE),
            f64::from_bits(crate::value::INT32_TAG | 5), // int32 5
            f64::from_bits(crate::value::STRING_TAG | 0x1000), // string tag (addr never deref'd)
            f64::from_bits(crate::value::BIGINT_TAG | 0x1000), // bigint tag (addr never deref'd)
            // #10479: inline SSO strings ("uri", "a") and a synthetic class ref.
            f64::from_bits(crate::value::SHORT_STRING_TAG | 0x0300_0069_7275),
            f64::from_bits(crate::value::SHORT_STRING_TAG | 0x0100_0000_0061),
            f64::from_bits(crate::value::INT32_TAG | 0x8000_0000),
            // Ordinary numbers: a dynamic `1.5 instanceof Number` reached the
            // walk and matched through the ToObject wrapper.
            1.5,
            -0.0,
            f64::NAN,
        ];
        for lhs in cases {
            // A dummy non-object RHS is never consulted for a non-object LHS.
            assert!(
                !ordinary_has_instance_prototype_walk(lhs, 0.0),
                "primitive LHS {:#018x} must not be instanceof anything",
                lhs.to_bits()
            );
        }
    }
}

/// #7575 — the generic-origin edge in [`class_chain_reaches`].
///
/// Perry monomorphizes generic classes, so `new Gen<number>()` stamps its
/// instance with `Gen$num`'s id while `x instanceof Gen` asks about the
/// GENERIC's id. These exercise the walk directly over synthetic ids: the
/// gap test proves the end-to-end behaviour, this proves the walk's shape,
/// including that the new edge does not make unrelated classes match.
#[cfg(test)]
mod generic_origin_chain_tests {
    use super::class_chain_reaches;
    use crate::object::{js_register_class_generic_origin, js_register_class_parent};

    // Ids well outside the reserved 0xFFFF00xx band and outside any range a
    // compiled module hands out in a test run.
    const BASE: u32 = 0x0757_5000;
    const GENERIC: u32 = 0x0757_5001;
    const SPECIALIZED: u32 = 0x0757_5002;
    const OTHER_SPECIALIZED: u32 = 0x0757_5003;
    const SUB_GENERIC: u32 = 0x0757_5004;
    const SUB_SPECIALIZED: u32 = 0x0757_5005;
    const UNRELATED: u32 = 0x0757_5006;

    fn wire() {
        // class Base {}
        // class Generic<T> extends Base {}        -> Specialized, OtherSpecialized
        // class SubGeneric<T> extends Generic<T> {} -> SubSpecialized
        js_register_class_parent(GENERIC, BASE);
        js_register_class_parent(SPECIALIZED, BASE);
        js_register_class_generic_origin(SPECIALIZED, GENERIC);
        js_register_class_parent(OTHER_SPECIALIZED, BASE);
        js_register_class_generic_origin(OTHER_SPECIALIZED, GENERIC);
        js_register_class_parent(SUB_GENERIC, GENERIC);
        js_register_class_parent(SUB_SPECIALIZED, GENERIC);
        js_register_class_generic_origin(SUB_SPECIALIZED, SUB_GENERIC);
    }

    #[test]
    fn a_specialization_is_an_instance_of_its_generic_and_of_the_real_base() {
        wire();
        // The bug: this was false, because GENERIC is in no parent chain.
        assert!(class_chain_reaches(SPECIALIZED, GENERIC));
        assert!(class_chain_reaches(SPECIALIZED, BASE));
        assert!(class_chain_reaches(SPECIALIZED, SPECIALIZED));
    }

    #[test]
    fn the_origin_edge_walks_the_generics_own_parents() {
        wire();
        // SubSpecialized -> (origin) SubGeneric -> (parent) Generic -> Base.
        assert!(class_chain_reaches(SUB_SPECIALIZED, SUB_GENERIC));
        assert!(class_chain_reaches(SUB_SPECIALIZED, GENERIC));
        assert!(class_chain_reaches(SUB_SPECIALIZED, BASE));
    }

    /// The edge must not make everything match everything: two specializations
    /// of the same generic are siblings, not ancestors of one another, and the
    /// generic is not an instance of its own specialization.
    #[test]
    fn the_origin_edge_stays_directional_and_does_not_widen_matches() {
        wire();
        assert!(!class_chain_reaches(SPECIALIZED, OTHER_SPECIALIZED));
        assert!(!class_chain_reaches(OTHER_SPECIALIZED, SPECIALIZED));
        assert!(!class_chain_reaches(GENERIC, SPECIALIZED));
        assert!(!class_chain_reaches(BASE, GENERIC));
        assert!(!class_chain_reaches(SPECIALIZED, UNRELATED));
        assert!(!class_chain_reaches(UNRELATED, GENERIC));
        assert!(!class_chain_reaches(SPECIALIZED, 0));
        assert!(!class_chain_reaches(0, GENERIC));
    }

    /// A self-edge or a cycle must terminate rather than hang: the walk is
    /// depth-bounded and refuses `cur -> cur`.
    #[test]
    fn a_self_referential_registration_terminates() {
        js_register_class_generic_origin(0x0757_50FF, 0x0757_50FF); // rejected
        js_register_class_parent(0x0757_50FE, 0x0757_50FE); // self parent
        assert!(!class_chain_reaches(0x0757_50FE, GENERIC));
        assert!(class_chain_reaches(0x0757_50FE, 0x0757_50FE));
    }
}
