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

/// Built-in constructor / namespace identifiers that should resolve to
/// a real `globalThis.<Name>` closure pointer when used as a bare
/// expression value (e.g. `inst.constructor === Date`, drizzle's
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
        "Array"
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
            | "Float32Array"
            | "Float64Array"
            | "BigInt64Array"
            | "BigUint64Array"
            | "Uint8ClampedArray"
            | "process"
    )
}
