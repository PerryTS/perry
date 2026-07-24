/// Check if a name is a built-in global function provided by the runtime.
/// #1454: setImmediate/clearImmediate recognized too, so bare calls lower to
/// ExternFuncRef (codegen fast path), not a not-a-function GlobalGet.
pub(crate) fn is_builtin_function(name: &str) -> bool {
    matches!(
        name,
        "setTimeout"
            | "setInterval"
            | "setImmediate"
            | "clearTimeout"
            | "clearInterval"
            | "clearImmediate"
            | "gc"
    )
}

/// Built-in constructor / namespace identifiers, plus `globalThis` itself,
/// that should resolve to a real `globalThis.<Name>` value when used as a
/// bare expression value (e.g. `inst.constructor === Date`, drizzle's
/// `value.constructor === Object`, lodash's `var A = context.Array`).
/// Mirrors `populate_global_this_builtins` in
/// `crates/perry-runtime/src/object.rs` and
/// `is_global_this_builtin_name` in `crates/perry-codegen/src/expr.rs`.
///
/// Callable surfaces — `Date()`/`new Date()`/`Date.now()`/`Math.PI` —
/// are intercepted by dedicated HIR variants (`Expr::DateNow`,
/// `Expr::DateNew`, `Expr::DateGet*`, `Expr::MathPow`, …) before the
/// ident lowering reaches this point, so converting bare names to
/// `PropertyGet { GlobalGet, name }` doesn't disturb those paths.
pub(crate) fn is_builtin_global_value_name(name: &str) -> bool {
    matches!(
        name,
        "globalThis"
            // #4511: Node's `global` alias for the global object. Resolves to
            // `globalThis.global`, a self-reference installed in
            // `populate_global_this_builtins`.
            | "global"
            | "Array"
            | "Object"
            | "String"
            | "Number"
            | "Boolean"
            | "Function"
            | "RegExp"
            | "Date"
            | "Error"
            | "TypeError"
            | "RangeError"
            | "SyntaxError"
            | "ReferenceError"
            | "EvalError"
            | "URIError"
            | "AggregateError"
            | "Symbol"
            | "Promise"
            | "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "WeakRef"
            | "Proxy"
            | "BigInt"
            | "Uint8Array"
            | "Int8Array"
            | "Uint16Array"
            | "Int16Array"
            | "Uint32Array"
            | "Int32Array"
            | "Float16Array"
            | "Float32Array"
            | "Float64Array"
            | "BigInt64Array"
            | "BigUint64Array"
            | "Uint8ClampedArray"
            | "ArrayBuffer"
            | "SharedArrayBuffer"
            | "DataView"
            | "TextEncoder"
            | "TextDecoder"
            | "TextEncoderStream"
            | "TextDecoderStream"
            | "CompressionStream"
            | "DecompressionStream"
            | "Navigator"
            | "URL"
            | "URLSearchParams"
            | "URLPattern"
            | "AbortController"
            | "AbortSignal"
            | "EventTarget"
            | "Crypto"
            | "CryptoKey"
            | "SubtleCrypto"
            | "Event"
            | "CustomEvent"
            | "DOMException"
            | "FormData"
            | "Blob"
            | "File"
            | "Headers"
            | "Request"
            | "Response"
            | "MessageChannel"
            | "MessagePort"
            | "BroadcastChannel"
            | "Storage"
            | "localStorage"
            | "sessionStorage"
            | "WebSocket"
            | "FinalizationRegistry"
            | "Performance"
            | "PerformanceEntry"
            | "PerformanceMark"
            | "PerformanceMeasure"
            | "PerformanceObserver"
            | "PerformanceObserverEntryList"
            | "PerformanceResourceTiming"
            // #2875: TC39 explicit-resource-management globals.
            | "DisposableStack"
            | "AsyncDisposableStack"
            | "SuppressedError"
            | "Buffer"
            | "fetch"
            | "process"
            | "console"
            | "Math"
            | "JSON"
            | "Reflect"
            | "Atomics"
            // Test262 installs `globalThis.print`; bare `print(...)` must
            // resolve through the global object instead of the unknown-ident
            // numeric fallback.
            | "print"
            | "crypto"
            | "WebAssembly"
            // TC39 Temporal namespace (#4686): a bare `Temporal` (and the
            // member-access base in `Temporal.Duration`) resolves to the
            // installed `globalThis.Temporal` namespace object.
            | "Temporal"
            // #3579: `var myEval = eval; myEval(...)` needs the same callable
            // globalThis function value as direct `globalThis.eval` reads.
            | "eval"
            // #2905: standard global helper functions used as bare values
            // (`const p = parseInt`). Bare CALLS (`parseInt(x)`) are picked
            // off earlier by `try_global_builtins` → `Expr::ParseInt`/etc., so
            // these only fire for value reads.
            | "parseInt"
            | "parseFloat"
            | "isNaN"
            | "isFinite"
            | "encodeURI"
            | "decodeURI"
            | "encodeURIComponent"
            | "decodeURIComponent"
            // #4511: legacy escape/unescape (ES Annex B). Bare calls and value
            // reads both resolve through `globalThis.<name>` → the runtime
            // thunk (no dedicated HIR variant). Used by `qs` (→ `stripe`).
            | "escape"
            | "unescape"
            // #5015: callable global helpers that have a `globalThis.<name>`
            // thunk and are recognized as known globals + folded to typeof
            // "function" (#3986), but were never added here — so a bare *value*
            // read (`const m = queueMicrotask`, `{ scheduleMicrotask:
            // queueMicrotask }`) fell through to the `GlobalGet(0)` sentinel and
            // evaluated to the number `0`, not a function. react-reconciler's
            // host config (`scheduleMicrotask: queueMicrotask`) hit exactly this:
            // `updateContainerSync` → `scheduleImmediateRootScheduleTask` called
            // the stored `0` and threw "value is not a function". Direct CALLS
            // (`queueMicrotask(fn)`, `btoa(s)`) are picked off earlier in
            // expr_call/globals.rs, so this only affects value reads.
            | "queueMicrotask"
            | "structuredClone"
            | "atob"
            | "btoa"
    )
}

/// Spec-defined `.length` (declared-parameter count) of a built-in
/// *constructor*, used by the `<Builtin>.length` HIR fold (#3143). Built-in
/// constructors are backed by a shared no-op closure thunk in the runtime
/// (`global_this_builtin_noop_thunk`) with no per-constructor arity recorded,
/// so a value-read of `.length` returns 0 instead of the spec count
/// (`Array.length === 1`, `Date.length === 7`, …). Folding the read here at
/// lowering time yields the spec constant directly.
///
/// Returns `None` for names that are *not* constructors tracked by this
/// table (`globalThis`, `process`, `console`, …) — those keep their existing
/// lowering so the read falls through to the runtime unchanged.
///
/// Only consulted when the receiver lowered to bare `GlobalGet(0)` (no local
/// shadowing), exactly mirroring the `.name` fold's gating.
pub(crate) fn builtin_constructor_length(name: &str) -> Option<u32> {
    let len = match name {
        "Array" | "Object" | "String" | "Number" | "Boolean" | "Function" | "Error"
        | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "EvalError"
        | "URIError" | "Promise" | "WeakRef" | "BigInt" | "Event" | "CustomEvent" | "WebSocket" => {
            1
        }
        "Symbol" | "Map" | "Set" | "WeakMap" | "WeakSet" | "MessageChannel" | "MessagePort"
        | "Navigator" | "TextEncoderStream" | "TextDecoderStream" | "DOMException" | "Storage"
        | "FormData" => 0,
        "CompressionStream" | "DecompressionStream" => 1,
        "RegExp" | "Proxy" | "File" => 2,
        "BroadcastChannel" => 1,
        "Date" => 7,
        "Uint8Array" | "Int8Array" | "Uint16Array" | "Int16Array" | "Uint32Array"
        | "Int32Array" | "Float16Array" | "Float32Array" | "Float64Array" | "BigInt64Array"
        | "BigUint64Array" | "Uint8ClampedArray" => 3,
        _ => return None,
    };
    Some(len)
}

#[cfg(test)]
mod tests {
    use super::builtin_constructor_length;

    #[test]
    fn form_data_constructor_has_zero_declared_parameters() {
        assert_eq!(builtin_constructor_length("FormData"), Some(0));
    }
}

/// Spec-defined `.length` for callable global function values that lower as
/// `globalThis.<name>` reads when used as bare values.
pub(crate) fn builtin_global_function_length(name: &str) -> Option<u32> {
    let len = match name {
        "structuredClone" => 2,
        "setTimeout" | "setInterval" | "parseInt" => 2,
        "eval" | "fetch" | "atob" | "btoa" | "clearTimeout" | "clearInterval" | "setImmediate"
        | "clearImmediate" | "queueMicrotask" | "parseFloat" | "isNaN" | "isFinite"
        | "encodeURI" | "decodeURI" | "encodeURIComponent" | "decodeURIComponent" | "escape"
        | "unescape" => 1,
        _ => return None,
    };
    Some(len)
}

/// Spec-defined static *function* members of built-in namespaces /
/// constructors. Used by the `<Builtin>.<member>.name` HIR fold (#2144)
/// to recognize cases where `.name` should resolve to the member ident
/// string — `Math.min.name === "min"`, `Promise.race.name === "race"`,
/// `Array.isArray.name === "isArray"`, and so on.
///
/// Only function-typed members are listed. Numeric constants (`Math.PI`,
/// `Number.EPSILON`) and namespace-level objects are excluded so that
/// reads like `Math.PI.name` continue to lower the normal way (which
/// yields `undefined` on a number, matching Node).
///
/// The list is conservative — adding a new pair is a one-line change and
/// the safe fallback (no fold → existing behavior) is the same shape as
/// pre-fix. Mirrors well-known spec surfaces; the hot ones are the ones
/// surfaced by `built-ins/Function` Test262 (38× `Cannot read … (reading
/// 'name')` on the #799 radar).
pub(crate) fn is_builtin_static_function_member(namespace: &str, member: &str) -> bool {
    match namespace {
        "Math" => matches!(
            member,
            "abs"
                | "acos"
                | "acosh"
                | "asin"
                | "asinh"
                | "atan"
                | "atan2"
                | "atanh"
                | "cbrt"
                | "ceil"
                | "clz32"
                | "cos"
                | "cosh"
                | "exp"
                | "expm1"
                | "floor"
                | "fround"
                | "f16round"
                | "hypot"
                | "imul"
                | "log"
                | "log10"
                | "log1p"
                | "log2"
                | "max"
                | "min"
                | "pow"
                | "random"
                | "round"
                | "sign"
                | "sin"
                | "sinh"
                | "sqrt"
                | "tan"
                | "tanh"
                | "trunc"
        ),
        "Promise" => matches!(
            member,
            "resolve" | "reject" | "all" | "race" | "allSettled" | "any" | "withResolvers" | "try"
        ),
        "Array" => matches!(member, "isArray" | "from" | "of" | "fromAsync"),
        "Object" => matches!(
            member,
            "assign"
                | "create"
                | "defineProperties"
                | "defineProperty"
                | "entries"
                | "freeze"
                | "fromEntries"
                | "getOwnPropertyDescriptor"
                | "getOwnPropertyDescriptors"
                | "getOwnPropertyNames"
                | "getOwnPropertySymbols"
                | "getPrototypeOf"
                | "groupBy"
                | "hasOwn"
                | "is"
                | "isExtensible"
                | "isFrozen"
                | "isSealed"
                | "keys"
                | "preventExtensions"
                | "seal"
                | "setPrototypeOf"
                | "values"
        ),
        "Number" => matches!(
            member,
            "isFinite" | "isInteger" | "isNaN" | "isSafeInteger" | "parseFloat" | "parseInt"
        ),
        "String" => matches!(member, "fromCharCode" | "fromCodePoint" | "raw"),
        "Symbol" => matches!(member, "for" | "keyFor"),
        "JSON" => matches!(member, "parse" | "stringify"),
        "Date" => matches!(member, "now" | "UTC" | "parse"),
        "ArrayBuffer" => matches!(member, "isView"),
        "BigInt" => matches!(member, "asIntN" | "asUintN"),
        "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
        | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
        | "BigInt64Array" | "BigUint64Array" => matches!(member, "from" | "of"),
        "Reflect" => matches!(
            member,
            "apply"
                | "construct"
                | "defineProperty"
                | "deleteProperty"
                | "get"
                | "getOwnPropertyDescriptor"
                | "getPrototypeOf"
                | "has"
                | "isExtensible"
                | "ownKeys"
                | "preventExtensions"
                | "set"
                | "setPrototypeOf"
        ),
        "Atomics" => matches!(
            member,
            "load"
                | "isLockFree"
                | "store"
                | "add"
                | "sub"
                | "and"
                | "or"
                | "xor"
                | "exchange"
                | "compareExchange"
                | "notify"
                | "wait"
                | "waitAsync"
        ),
        "WebAssembly" => matches!(
            member,
            "compile" | "compileStreaming" | "instantiate" | "instantiateStreaming" | "validate"
        ),
        _ => false,
    }
}

/// Spec-defined `.length` for static *function* members recognized by
/// [`is_builtin_static_function_member`].
///
/// Perry currently lowers many builtin static functions as intrinsics rather
/// than first-class closure values, so direct `Builtin.method.length` reads and
/// compile-time-folded descriptor probes need this central table just like
/// constructors use [`builtin_constructor_length`].
pub(crate) fn builtin_static_function_length(namespace: &str, member: &str) -> Option<u32> {
    let len = match namespace {
        "Math" => match member {
            "random" => 0,
            "atan2" | "hypot" | "imul" | "max" | "min" | "pow" => 2,
            "abs" | "acos" | "acosh" | "asin" | "asinh" | "atan" | "atanh" | "cbrt" | "ceil"
            | "clz32" | "cos" | "cosh" | "exp" | "expm1" | "floor" | "fround" | "f16round"
            | "log" | "log10" | "log1p" | "log2" | "round" | "sign" | "sin" | "sinh" | "sqrt"
            | "tan" | "tanh" | "trunc" => 1,
            _ => return None,
        },
        "Promise" => match member {
            "withResolvers" => 0,
            "resolve" | "reject" | "all" | "race" | "allSettled" | "any" | "try" => 1,
            _ => return None,
        },
        "Array" => match member {
            "of" => 0,
            "isArray" | "from" | "fromAsync" => 1,
            _ => return None,
        },
        "Object" => match member {
            "defineProperty" => 3,
            "assign"
            | "create"
            | "defineProperties"
            | "groupBy"
            | "hasOwn"
            | "is"
            | "getOwnPropertyDescriptor"
            | "setPrototypeOf" => 2,
            "entries"
            | "freeze"
            | "fromEntries"
            | "getOwnPropertyDescriptors"
            | "getOwnPropertyNames"
            | "getOwnPropertySymbols"
            | "getPrototypeOf"
            | "isExtensible"
            | "isFrozen"
            | "isSealed"
            | "keys"
            | "preventExtensions"
            | "seal"
            | "values" => 1,
            _ => return None,
        },
        "Number" => match member {
            "parseInt" => 2,
            "isFinite" | "isInteger" | "isNaN" | "isSafeInteger" | "parseFloat" => 1,
            _ => return None,
        },
        "String" => match member {
            "fromCharCode" | "fromCodePoint" | "raw" => 1,
            _ => return None,
        },
        "Symbol" => match member {
            "for" | "keyFor" => 1,
            _ => return None,
        },
        "JSON" => match member {
            "parse" => 2,
            "stringify" => 3,
            _ => return None,
        },
        "Date" => match member {
            "now" => 0,
            "UTC" => 7,
            "parse" => 1,
            _ => return None,
        },
        "ArrayBuffer" => match member {
            "isView" => 1,
            _ => return None,
        },
        "BigInt" => match member {
            "asIntN" | "asUintN" => 2,
            _ => return None,
        },
        "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
        | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
        | "BigInt64Array" | "BigUint64Array" => match member {
            "from" => 1,
            "of" => 0,
            _ => return None,
        },
        "Reflect" => match member {
            "apply" | "defineProperty" | "set" => 3,
            "construct"
            | "deleteProperty"
            | "get"
            | "getOwnPropertyDescriptor"
            | "has"
            | "setPrototypeOf" => 2,
            "getPrototypeOf" | "isExtensible" | "ownKeys" | "preventExtensions" => 1,
            _ => return None,
        },
        "Atomics" => match member {
            "load" => 2,
            "isLockFree" => 1,
            "store" | "add" | "sub" | "and" | "or" | "xor" | "exchange" => 3,
            "compareExchange" => 4,
            "notify" => 3,
            "wait" => 4,
            "waitAsync" => 4,
            _ => return None,
        },
        "WebAssembly" => match member {
            "compile"
            | "compileStreaming"
            | "instantiate"
            | "instantiateStreaming"
            | "validate" => 1,
            _ => return None,
        },
        _ => return None,
    };
    Some(len)
}
