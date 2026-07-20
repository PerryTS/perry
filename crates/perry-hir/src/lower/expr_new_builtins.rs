//! Built-in / native-module constructor-name resolution helpers for `new`
//! lowering. Extracted from `expr_new.rs` to keep that file under the
//! 2000-line cap (#5253). Pure mechanical move — no behavior change; the two
//! functions are leaf lookups consulted by `expr_new::lower_new`.

use super::LoweringContext;

/// Map a `(module, export)` pair to the canonical built-in constructor name
/// `lower_new` uses (URL/TextEncoder/stream-web wrappers, EventEmitter*).
pub(super) fn module_constructor_name(
    module_name: &str,
    method_name: Option<&str>,
) -> Option<&'static str> {
    match (module_name, method_name) {
        ("events", Some("EventEmitterAsyncResource")) => Some("EventEmitterAsyncResource"),
        ("url", Some("URL")) => Some("URL"),
        ("url", Some("URLSearchParams")) => Some("URLSearchParams"),
        ("url", Some("URLPattern")) => Some("URLPattern"),
        ("util", Some("TextEncoder")) => Some("TextEncoder"),
        ("util", Some("TextDecoder")) => Some("TextDecoder"),
        ("stream/web", Some("TextEncoderStream"))
        | ("node:stream/web", Some("TextEncoderStream")) => Some("TextEncoderStream"),
        ("stream/web", Some("TextDecoderStream"))
        | ("node:stream/web", Some("TextDecoderStream")) => Some("TextDecoderStream"),
        ("stream/web", Some("CompressionStream"))
        | ("node:stream/web", Some("CompressionStream")) => Some("CompressionStream"),
        ("stream/web", Some("DecompressionStream"))
        | ("node:stream/web", Some("DecompressionStream")) => Some("DecompressionStream"),
        _ => None,
    }
}

/// Names on the global object that are real JS / Web built-in *constructors*
/// (`typeof === "function"`, invokable with `new`). Consulted by `lower_new`
/// to re-dispatch `new globalThis.<Ctor>(...)` through the bare-identifier
/// `new <Ctor>(...)` path so a member-expression callee constructs the exact
/// same intrinsic as the plain form (#6726: `new globalThis.Set()` used to
/// fall through to an empty-object placeholder with no `.has`, because the
/// data-structure builtins have a dedicated HIR variant — `Expr::SetNew`,
/// `MapNew`, `DateNew`, … — but no codegen `lower_builtin_new` arm for the
/// member-callee reroute to land on).
///
/// Symbol/BigInt/Math/JSON are intentionally omitted: `new globalThis.Symbol()`
/// already throws the correct "not a constructor" TypeError via the
/// non-identifier path, so re-routing them here would be redundant. URL /
/// TextEncoder / the fetch + worker-messaging constructors are handled earlier
/// by `lower_new_member_native` and never reach the re-dispatch, but are listed
/// for completeness since routing them through the bare-ident path is
/// equivalent. Kept in step with codegen's `is_global_this_builtin_function_name`
/// (perry-codegen cannot be imported from perry-hir); over-inclusion is harmless
/// because an unrecognized name still lowers to the generic `Expr::New` the bare
/// form uses.
pub(super) fn is_reified_global_builtin_constructor(name: &str) -> bool {
    matches!(
        name,
        "Object"
            | "Array"
            | "Function"
            | "Boolean"
            | "Number"
            | "String"
            | "Date"
            | "RegExp"
            | "Error"
            | "TypeError"
            | "RangeError"
            | "ReferenceError"
            | "SyntaxError"
            | "EvalError"
            | "URIError"
            | "AggregateError"
            | "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "WeakRef"
            | "Promise"
            | "Proxy"
            | "FinalizationRegistry"
            | "ArrayBuffer"
            | "SharedArrayBuffer"
            | "DataView"
            | "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float16Array"
            | "Float32Array"
            | "Float64Array"
            | "BigInt64Array"
            | "BigUint64Array"
            | "URL"
            | "URLSearchParams"
            | "URLPattern"
            | "TextEncoder"
            | "TextDecoder"
            | "TextEncoderStream"
            | "TextDecoderStream"
            | "CompressionStream"
            | "DecompressionStream"
            | "ReadableStream"
            | "WritableStream"
            | "TransformStream"
            | "AbortController"
            | "AbortSignal"
            | "EventTarget"
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
            | "WebSocket"
            | "DisposableStack"
            | "AsyncDisposableStack"
            | "SuppressedError"
    )
}

/// Resolve `new <obj>.<prop>()` against the global object or a built-in /
/// native module alias to a canonical built-in constructor name.
pub(super) fn global_member_constructor_name(
    ctx: &LoweringContext,
    obj_name: &str,
    prop_name: &str,
) -> Option<&'static str> {
    if obj_name == "globalThis" && ctx.lookup_local("globalThis").is_none() {
        return match prop_name {
            "URL" => Some("URL"),
            "URLSearchParams" => Some("URLSearchParams"),
            "URLPattern" => Some("URLPattern"),
            "TextEncoder" => Some("TextEncoder"),
            "TextDecoder" => Some("TextDecoder"),
            "MessageChannel" => Some("MessageChannel"),
            "BroadcastChannel" => Some("BroadcastChannel"),
            "TextEncoderStream" => Some("TextEncoderStream"),
            "TextDecoderStream" => Some("TextDecoderStream"),
            "CompressionStream" => Some("CompressionStream"),
            "DecompressionStream" => Some("DecompressionStream"),
            _ => None,
        };
    }

    if let Some(module_name) = ctx.lookup_builtin_module_alias(obj_name) {
        if let Some(name) = module_constructor_name(module_name, Some(prop_name)) {
            return Some(name);
        }
    }
    if let Some((module_name, None)) = ctx.lookup_native_module(obj_name) {
        if let Some(name) = module_constructor_name(module_name, Some(prop_name)) {
            return Some(name);
        }
    }
    None
}
