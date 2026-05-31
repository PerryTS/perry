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
            | "fetch"
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
            | "URL"
            | "URLSearchParams"
            | "AbortController"
            | "AbortSignal"
            | "EventTarget"
            | "FormData"
            | "Blob"
            | "File"
            | "Headers"
            | "Request"
            | "Response"
            | "FinalizationRegistry"
            // #2875: TC39 explicit-resource-management globals.
            | "DisposableStack"
            | "AsyncDisposableStack"
            | "SuppressedError"
            | "Buffer"
            | "process"
            | "console"
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
/// Returns `None` for names that are *not* standard constructors with a
/// well-defined `.length` (`globalThis`, `process`, `console`, `Buffer`,
/// `TextEncoderStream`, `TextDecoderStream`) — those keep their existing
/// lowering so the read falls through to the runtime unchanged.
///
/// Only consulted when the receiver lowered to bare `GlobalGet(0)` (no local
/// shadowing), exactly mirroring the `.name` fold's gating.
pub(crate) fn builtin_constructor_length(name: &str) -> Option<u32> {
    let len = match name {
        "Array" | "Object" | "String" | "Number" | "Boolean" | "Function" | "Error"
        | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "EvalError"
        | "URIError" | "Promise" | "WeakRef" | "BigInt" => 1,
        "Symbol" | "Map" | "Set" | "WeakMap" | "WeakSet" => 0,
        "RegExp" | "Proxy" | "File" => 2,
        "Date" => 7,
        "Uint8Array" | "Int8Array" | "Uint16Array" | "Int16Array" | "Uint32Array"
        | "Int32Array" | "Float16Array" | "Float32Array" | "Float64Array" | "BigInt64Array"
        | "BigUint64Array" | "Uint8ClampedArray" => 3,
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
        _ => false,
    }
}
