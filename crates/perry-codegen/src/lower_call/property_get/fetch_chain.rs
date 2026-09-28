//! Chained Web Fetch
//! (`r.headers.get(k)`, `r.clone().status`, `new Response(...).text()`).

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::FnCtx;

// Reach the dispatch helpers (`pub(in crate::lower_call)` / `pub(super)`) by
// their canonical crate-relative paths — they live in sibling modules of the
// `lower_call` parent.
use crate::lower_call::options::lower_fetch_native_method;

/// Chained Web Fetch dispatch. Returns `Ok(Some(_))` when it claims the call.
pub(crate) fn try_lower_fetch_chain(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<Option<String>> {
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
