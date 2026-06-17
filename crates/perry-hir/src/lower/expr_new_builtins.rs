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
