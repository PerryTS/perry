//! AbortController / AbortSignal / EventTarget dispatch + chained Web Fetch
//! (`r.headers.get(k)`, `r.clone().status`, `new Response(...).text()`).
//! Pure code move from `property_get.rs` — no behavior change.

use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx};
use crate::lower_array_method::lower_array_method;
use crate::lower_string_method::{is_known_string_method_name, lower_string_method};
use crate::nanbox::double_literal;
use crate::type_analysis::{
    is_array_expr, is_global_constructor_expr, is_map_expr, is_native_module_dynamic_index,
    is_promise_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I32, I64};

// Reach the dispatch helpers (`pub(in crate::lower_call)` / `pub(super)`) by
// their canonical crate-relative paths — they live in sibling modules of the
// `lower_call` parent.
use crate::lower_call::event_target::lower_event_target_call;
use crate::lower_call::options::lower_abort_controller_call;
use crate::lower_call::options::lower_fetch_native_method;

/// AbortController / AbortSignal / EventTarget method calls, then chained Web
/// Fetch dispatch. Returns `Ok(Some(_))` when any of these claims the call.
pub(crate) fn try_lower_fetch_chain(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    // ── AbortController / AbortSignal dispatch ──
    // `new AbortController()` returns a NaN-boxed pointer
    // (refined to `Named("AbortController")`). The runtime's
    // ObjectHeader carries `signal` / `aborted` fields that the
    // generic property-get path reads. Method calls need explicit
    // interception because the class isn't in `ctx.classes`.
    if let Some(val) = lower_abort_controller_call(ctx, object, property, args)? {
        return Ok(Some(val));
    }

    if let Some(val) = lower_event_target_call(ctx, object, property, args)? {
        return Ok(Some(val));
    }

    // ── Chained Web Fetch dispatch ──
    // `r.headers.get(k)` — the inner `r.headers` lowered to a
    // NativeMethodCall that returns an f64 Headers handle; route
    // the outer `.get(...)` (and friends) through the Headers FFI.
    // `r.clone().status` / `.text()` / etc — the inner clone call
    // returns an f64 Response handle; route the outer call through
    // the fetch dispatch.
    //
    // `new Response(...).text()` — likewise, when the receiver is
    // a direct `Expr::New { class_name: "Response"|"Headers"|"Request" }`
    // (no intermediate let binding).
    if let Expr::NativeMethodCall {
        module: chain_mod,
        method: chain_method,
        ..
    } = object
    {
        // Chain `<Response>.headers.<method>(...)` where chain_method == "headers".
        if chain_mod == "fetch" && chain_method == "headers" {
            if let Some(val) =
                lower_fetch_native_method(ctx, "Headers", property, Some(object), args)?
            {
                return Ok(Some(val));
            }
        }
        // Chain `<Response>.clone().<method>(...)` — dispatch as a
        // fetch method on the cloned handle.
        if chain_mod == "fetch" && chain_method == "clone" {
            if let Some(val) =
                lower_fetch_native_method(ctx, "fetch", property, Some(object), args)?
            {
                return Ok(Some(val));
            }
        }
    }
    // Chain `new Response(...).text()` / `.json()` etc.
    if let Expr::New { class_name: nc, .. } = object {
        // #6003: `new Headers().set(...)` only dispatches through the fetch
        // FFI when the name still refers to the built-in — a user-defined
        // `class Headers`/`Response`/`Request` constructs the user class,
        // whose methods resolve via the ordinary class dispatch.
        let fetch_dispatch = matches!(nc.as_str(), "Response" | "Headers" | "Request")
            && !ctx.classes.contains_key(nc.as_str());
        if fetch_dispatch {
            let module = match nc.as_str() {
                "Response" => "fetch",
                "Headers" => "Headers",
                "Request" => "Request",
                _ => unreachable!(),
            };
            if let Some(val) = lower_fetch_native_method(ctx, module, property, Some(object), args)?
            {
                return Ok(Some(val));
            }
        }
    }
    Ok(None)
}
