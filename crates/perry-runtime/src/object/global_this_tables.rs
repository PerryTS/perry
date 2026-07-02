//! Static `globalThis` constructor/function metadata.

/// JS built-in constructor names exposed on `globalThis`. Pre-populated by
/// the singleton init in `js_get_global_this` so libraries that read these
/// off the global (lodash's `var Array = context.Array; var arrayProto =
/// Array.prototype`, the same `(globalThis as any).X` read shape) see a
/// non-undefined backing object. Codegen mirrors this list in
/// `perry-codegen/src/expr.rs::is_global_this_builtin_name` to decide when
/// `globalThis.<Name>` should route through the singleton instead of the
/// legacy `0.0` no-value placeholder.
pub(crate) const GLOBAL_THIS_BUILTIN_CONSTRUCTORS: &[&str] = &[
    "Array",
    "Object",
    "String",
    "Number",
    "Boolean",
    "Function",
    "RegExp",
    "Date",
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "ReferenceError",
    "EvalError",
    "URIError",
    "AggregateError",
    "Symbol",
    "Promise",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "WeakRef",
    "Proxy",
    "BigInt",
    "Uint8Array",
    "Int8Array",
    "Uint16Array",
    "Int16Array",
    "Uint32Array",
    "Int32Array",
    "Float16Array",
    "Float32Array",
    "Float64Array",
    "Uint8ClampedArray",
    "BigInt64Array",
    "BigUint64Array",
    "ArrayBuffer",
    "SharedArrayBuffer",
    "DataView",
    "TextEncoder",
    "TextDecoder",
    "TextEncoderStream",
    "TextDecoderStream",
    "CompressionStream",
    "DecompressionStream",
    "Navigator",
    "URL",
    "URLSearchParams",
    "URLPattern",
    "AbortController",
    "AbortSignal",
    "EventTarget",
    "Crypto",
    "CryptoKey",
    "SubtleCrypto",
    "Event",
    "CustomEvent",
    "DOMException",
    "FormData",
    "Blob",
    "File",
    "Headers",
    "Request",
    "Response",
    "MessageChannel",
    "MessagePort",
    "BroadcastChannel",
    "Storage",
    "WebSocket",
    "FinalizationRegistry",
    // #2875: TC39 explicit-resource-management globals. Backed by the
    // no-op constructor thunk so `typeof DisposableStack === "function"`;
    // real `new DisposableStack()` / `new SuppressedError(...)` flow through
    // codegen's `lower_builtin_new` to the dedicated runtime ctors.
    "DisposableStack",
    "AsyncDisposableStack",
    "SuppressedError",
    "Buffer",
];

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
