//! Static `globalThis` constructor/function metadata.

use super::global_this::*;

const NOOP_CONSTRUCTOR_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(global_this_builtin_noop_thunk, 1);
const TYPED_ARRAY_CONSTRUCTOR_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(typed_array_constructor_call_thunk, 1; with_declared(0));
const CONSTRUCT_ONLY_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(construct_only_builtin_call_thunk, 0);
const WEBCRYPTO_CONSTRUCTOR_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(webcrypto_illegal_constructor_thunk, 0);

/// A builtin's own declaration, including the CLASS identity of its instances
/// when its prototype occupies a class identity word. As with a member's
/// `JsFunctionInfo::flags`, installation consumes the declared fact; the
/// public name only labels the property that exposes the builtin.
#[derive(Clone, Copy)]
pub(crate) struct BuiltinConstructorDeclaration {
    pub name: &'static str,
    pub info: &'static crate::closure::JsFunctionInfo,
    pub prototype_class: Option<u32>,
    pub prototype_serial: Option<u64>,
}

macro_rules! builtin_constructors {
    ($($name:literal => $info:expr $(; class $class:expr)? $(; serial $serial:expr)?),* $(,)?) => {
        /// Builtin declarations, in the order captured by shared constructor bodies.
        pub(crate) const GLOBAL_THIS_BUILTIN_CONSTRUCTORS: &[BuiltinConstructorDeclaration] = &[
            $(BuiltinConstructorDeclaration {
                name: $name,
                // SAFETY: fn_info! returns its own static immutable record.
                info: unsafe { &*$info },
                prototype_class: builtin_constructors!(@optional $($class)?),
                prototype_serial: builtin_constructors!(@optional $($serial)?),
            },)*
        ];
    };
    (@optional $class:expr) => { Some($class) };
    (@optional) => { None };
}

// JS built-in constructor declarations exposed on `globalThis`. Pre-populated by
// the singleton init in `js_get_global_this` so libraries that read these
// off the global (lodash's `var Array = context.Array; var arrayProto =
// Array.prototype`, the same `(globalThis as any).X` read shape) see a
// non-undefined backing object. Codegen mirrors this list in
// `perry-codegen/src/expr.rs::is_global_this_builtin_name` to decide when
// `globalThis.<Name>` should route through the singleton instead of the
// legacy `0.0` no-value placeholder.
builtin_constructors! {
    "Array" => crate::fn_info!(global_this_array_thunk, 1; with_rest(0)),
    "Object" => NOOP_CONSTRUCTOR_INFO,
    "String" => crate::fn_info!(global_this_string_thunk, 1; with_declared(1)),
    "Number" => crate::fn_info!(global_this_number_thunk, 1; with_declared(1)),
    "Boolean" => crate::fn_info!(global_this_boolean_thunk, 1; with_declared(1)),
    "Function" => crate::fn_info!(unwind_in_tests global_this_function_call_thunk, 1; with_rest(0)); serial crate::closure::shape::INTRINSIC_SERIAL_FUNCTION,
    "RegExp" => crate::fn_info!(regexp_constructor_call_thunk, 2; with_declared(2)),
    "Date" => crate::fn_info!(global_this_date_thunk, 1; with_declared(1)),
    "Error" => crate::fn_info!(error_constructor_call_thunk, 1; with_declared(1)),
    "TypeError" => crate::fn_info!(type_error_constructor_call_thunk, 1; with_declared(1)),
    "RangeError" => crate::fn_info!(range_error_constructor_call_thunk, 1; with_declared(1)),
    "SyntaxError" => crate::fn_info!(syntax_error_constructor_call_thunk, 1; with_declared(1)),
    "ReferenceError" => crate::fn_info!(reference_error_constructor_call_thunk, 1; with_declared(1)),
    "EvalError" => crate::fn_info!(eval_error_constructor_call_thunk, 1; with_declared(1)),
    "URIError" => crate::fn_info!(uri_error_constructor_call_thunk, 1; with_declared(1)),
    "AggregateError" => NOOP_CONSTRUCTOR_INFO,
    "Symbol" => crate::fn_info!(global_this_symbol_thunk, 1; with_declared(1)),
    "Promise" => crate::fn_info!(promise_constructor_call_thunk, 1),
    "Map" => crate::fn_info!(map_constructor_call_thunk, 1),
    "Set" => crate::fn_info!(set_constructor_call_thunk, 1),
    "WeakMap" => crate::fn_info!(weak_map_constructor_call_thunk, 1); class crate::weakref::CLASS_ID_WEAKMAP,
    "WeakSet" => crate::fn_info!(weak_set_constructor_call_thunk, 1); class crate::weakref::CLASS_ID_WEAKSET,
    "WeakRef" => crate::fn_info!(weak_ref_constructor_call_thunk, 1); class crate::weakref::CLASS_ID_WEAKREF,
    "Proxy" => NOOP_CONSTRUCTOR_INFO,
    "BigInt" => crate::fn_info!(global_this_bigint_thunk, 1; with_declared(1)),
    "Uint8Array" => TYPED_ARRAY_CONSTRUCTOR_INFO,
    "Int8Array" => TYPED_ARRAY_CONSTRUCTOR_INFO,
    "Uint16Array" => TYPED_ARRAY_CONSTRUCTOR_INFO,
    "Int16Array" => TYPED_ARRAY_CONSTRUCTOR_INFO,
    "Uint32Array" => TYPED_ARRAY_CONSTRUCTOR_INFO,
    "Int32Array" => TYPED_ARRAY_CONSTRUCTOR_INFO,
    "Float16Array" => TYPED_ARRAY_CONSTRUCTOR_INFO,
    "Float32Array" => TYPED_ARRAY_CONSTRUCTOR_INFO,
    "Float64Array" => TYPED_ARRAY_CONSTRUCTOR_INFO,
    "Uint8ClampedArray" => TYPED_ARRAY_CONSTRUCTOR_INFO,
    "BigInt64Array" => TYPED_ARRAY_CONSTRUCTOR_INFO,
    "BigUint64Array" => TYPED_ARRAY_CONSTRUCTOR_INFO,
    "ArrayBuffer" => CONSTRUCT_ONLY_INFO,
    "SharedArrayBuffer" => CONSTRUCT_ONLY_INFO,
    "DataView" => CONSTRUCT_ONLY_INFO,
    "TextEncoder" => NOOP_CONSTRUCTOR_INFO,
    "TextDecoder" => NOOP_CONSTRUCTOR_INFO,
    "TextEncoderStream" => NOOP_CONSTRUCTOR_INFO,
    "TextDecoderStream" => NOOP_CONSTRUCTOR_INFO,
    "CompressionStream" => NOOP_CONSTRUCTOR_INFO,
    "DecompressionStream" => NOOP_CONSTRUCTOR_INFO,
    // The three core Web Streams constructors. Perry implements them (codegen
    // lowers `new ReadableStream(…)` and `x instanceof ReadableStream`, and the
    // class has an id), but the NAMES were never registered here or in codegen's
    // globalThis-builtin list — so a bare `ReadableStream` identifier resolved to
    // nothing: `typeof ReadableStream === "undefined"` and `"ReadableStream" in
    // globalThis` was false, whereas Node exposes all three as functions.
    // Libraries feature-detect precisely that (`typeof ReadableStream !==
    // "undefined" ? … : …`) when deciding how to consume a `fetch()` body.
    "ReadableStream" => NOOP_CONSTRUCTOR_INFO,
    "WritableStream" => NOOP_CONSTRUCTOR_INFO,
    "TransformStream" => NOOP_CONSTRUCTOR_INFO,
    "Navigator" => NOOP_CONSTRUCTOR_INFO,
    "URL" => NOOP_CONSTRUCTOR_INFO,
    "URLSearchParams" => NOOP_CONSTRUCTOR_INFO,
    "URLPattern" => crate::fn_info!(global_this_url_pattern_call_thunk, 2; with_declared(2)),
    "AbortController" => NOOP_CONSTRUCTOR_INFO,
    "AbortSignal" => NOOP_CONSTRUCTOR_INFO,
    "EventTarget" => NOOP_CONSTRUCTOR_INFO,
    "Crypto" => WEBCRYPTO_CONSTRUCTOR_INFO,
    "CryptoKey" => WEBCRYPTO_CONSTRUCTOR_INFO,
    "SubtleCrypto" => WEBCRYPTO_CONSTRUCTOR_INFO,
    "Event" => NOOP_CONSTRUCTOR_INFO,
    "CustomEvent" => NOOP_CONSTRUCTOR_INFO,
    "DOMException" => NOOP_CONSTRUCTOR_INFO,
    "FormData" => NOOP_CONSTRUCTOR_INFO,
    "Blob" => crate::fn_info!(global_this_blob_thunk, 2; with_declared(2)),
    "File" => crate::fn_info!(global_this_file_thunk, 3; with_declared(3)),
    "Headers" => crate::fn_info!(global_this_headers_thunk, 1; with_declared(1)),
    "Request" => crate::fn_info!(global_this_request_thunk, 2; with_declared(2)),
    "Response" => crate::fn_info!(global_this_response_thunk, 2; with_declared(2)),
    "MessageChannel" => crate::fn_info!(crate::messaging::js_message_channel_constructor_call_error, 0; with_declared(0)),
    "MessagePort" => crate::fn_info!(crate::messaging::js_message_port_constructor_call_error, 0; with_declared(0)),
    "BroadcastChannel" => crate::fn_info!(crate::messaging::js_broadcast_channel_constructor_call_error, 1; with_declared(1)),
    "Storage" => crate::fn_info!(crate::web_storage::storage_constructor_illegal, 0; with_declared(0)),
    "WebSocket" => NOOP_CONSTRUCTOR_INFO,
    "FinalizationRegistry" => NOOP_CONSTRUCTOR_INFO; class crate::weakref::CLASS_ID_FINALIZATION_REGISTRY,
    // #2875: TC39 explicit-resource-management globals. Backed by the
    // no-op constructor thunk so `typeof DisposableStack === "function"`;
    // real `new DisposableStack()` / `new SuppressedError(...)` flow through
    // codegen's `lower_builtin_new` to the dedicated runtime ctors.
    "DisposableStack" => NOOP_CONSTRUCTOR_INFO,
    "AsyncDisposableStack" => NOOP_CONSTRUCTOR_INFO,
    "SuppressedError" => NOOP_CONSTRUCTOR_INFO,
    "Buffer" => NOOP_CONSTRUCTOR_INFO,
}

/// Is `name` one of the built-in CONSTRUCTORS installed on `globalThis`?
///
/// #7518: `try_dispatch_value_called_proto_method` needs this to tell a built-in
/// *prototype method* invoked as a value — the #3716 uncurry-this idiom, which it
/// must re-dispatch by name on the call's `this` — from a global *constructor*
/// invoked as a value, which it must not: `this.EventTarget(…)` resolves
/// to nothing and the by-name tower's catch-all throws
/// `TypeError: EventTarget is not a function`.
///
/// Both are backed by the same `global_this_builtin_noop_thunk`, and the only
/// thing that used to separate them was the accident that constructors carried no
/// recorded builtin `.length`. That stopped being true as
/// [`builtin_constructor_spec_length`] below grew to cover them, so the
/// distinction has to be stated rather than inferred.
pub(crate) fn is_global_this_builtin_constructor_name(name: &str) -> bool {
    GLOBAL_THIS_BUILTIN_CONSTRUCTORS
        .iter()
        .any(|decl| decl.name == name)
}

/// #3655: spec `length` (declared-parameter count) for each built-in
/// constructor, so `Ctor.length` reads the right arity through the runtime
/// value path (`const C = DataView; C.length === 1`) and
/// `Object.getOwnPropertyDescriptor(Ctor, 'length').value` matches Node. The
/// HIR also folds bare `Ctor.length` constants (`analysis::builtin_constructor_length`);
/// these are the runtime fallback for rebound / passed-as-value constructors.
/// Values verified against `node --experimental-strip-types`. Unlisted names
/// fall through to the closure arity registry (0).
pub(crate) fn builtin_constructor_spec_length(name: &str) -> Option<u32> {
    let len = match name {
        "Symbol"
        | "Map"
        | "Set"
        | "WeakMap"
        | "WeakSet"
        | "TextEncoder"
        | "TextDecoder"
        | "TextEncoderStream"
        | "TextDecoderStream"
        | "URLSearchParams"
        | "URLPattern"
        | "AbortController"
        | "AbortSignal"
        | "EventTarget"
        | "DOMException"
        | "FormData"
        | "Blob"
        | "Headers"
        | "Response"
        | "MessageChannel"
        | "MessagePort"
        | "Storage"
        | "Navigator"
        | "DisposableStack"
        | "AsyncDisposableStack" => 0,
        "CompressionStream" | "DecompressionStream" => 1,
        "Array"
        | "Object"
        | "String"
        | "Number"
        | "Boolean"
        | "Function"
        | "Error"
        | "TypeError"
        | "RangeError"
        | "SyntaxError"
        | "ReferenceError"
        | "EvalError"
        | "URIError"
        | "WeakRef"
        | "BigInt"
        | "ArrayBuffer"
        | "SharedArrayBuffer"
        | "DataView"
        | "URL"
        | "Event"
        | "CustomEvent"
        | "Request"
        | "WebSocket"
        | "BroadcastChannel"
        | "FinalizationRegistry"
        | "Promise" => 1,
        "RegExp" | "Proxy" | "AggregateError" | "File" => 2,
        "Date" => 7,
        "SuppressedError" | "Buffer" | "Uint8Array" | "Int8Array" | "Uint16Array"
        | "Int16Array" | "Uint32Array" | "Int32Array" | "Float16Array" | "Float32Array"
        | "Float64Array" | "Uint8ClampedArray" | "BigInt64Array" | "BigUint64Array" => 3,
        _ => return None,
    };
    Some(len)
}

/// JS built-in namespaces (typeof === "object", not "function"). Same
/// shape on the singleton — a backing object with `prototype` so chained
/// reads degrade gracefully — but typeof reports "object".
pub(crate) const GLOBAL_THIS_BUILTIN_NAMESPACES: &[&str] = &[
    "console",
    "process",
    "Math",
    "JSON",
    "Reflect",
    "Atomics",
    "Intl",
    "WebAssembly",
    // TC39 Temporal (#4686): typeof Temporal === "object". The constructors
    // (Temporal.Duration, …) and the Temporal.Now sub-namespace are hung off it
    // by `install_temporal_namespace`.
    "Temporal",
];

/// JS global built-in functions exposed as function-valued properties on
/// `globalThis`. Unlike constructor sentinels, these call through to Perry's
/// real direct-call runtime helpers so rebinding works:
/// `const clone = globalThis.structuredClone; clone(value)`.
pub(crate) const GLOBAL_THIS_BUILTIN_FUNCTIONS: &[&str] = &[
    "eval",
    "fetch",
    "structuredClone",
    "atob",
    "btoa",
    "setTimeout",
    "clearTimeout",
    "setInterval",
    "clearInterval",
    "setImmediate",
    "clearImmediate",
    "queueMicrotask",
    // The `gc()` builtin, exposed as a real callable global value so the
    // capability-guard idioms `if (globalThis.gc) gc()` / `global.gc?.()` /
    // `const f = gc; f()` work (Perry's `gc()` is always available, unlike
    // Node's `--expose-gc`-gated one).
    "gc",
    // #2905: standard global helper functions. These route through Perry's
    // real direct-call runtime helpers, so `const p = parseInt; p("42px")`
    // and `globalThis.encodeURIComponent("a b")` match Node.
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "encodeURI",
    "decodeURI",
    "encodeURIComponent",
    "decodeURIComponent",
    // #4511: legacy escape/unescape (ES Annex B), used by `qs`.
    "escape",
    "unescape",
];

pub(crate) fn is_web_fetch_constructor(name: &str) -> bool {
    matches!(
        name,
        "Headers" | "Request" | "Response" | "Blob" | "File" | "FormData"
    )
}
