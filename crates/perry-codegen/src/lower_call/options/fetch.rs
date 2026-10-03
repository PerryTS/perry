//! Web Fetch API family lowering: Response / Headers / Request method
//! calls + property getters, plus axios / blob / readable_stream.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. Called before the generic
//! `lower_native_method_call` path so static factories
//! (`Response.json(v)`) also land here.

use anyhow::Result;
use perry_hir::Expr;

use super::get_raw_string_ptr;
use crate::expr::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, FnCtx};
use crate::nanbox::double_literal;
use crate::rooting::{any_operand_may_collect, with_rooted_group};
use crate::types::{DOUBLE, I1, I64};

/// Dispatch for the Web Fetch API family: Response/Headers/Request
/// methods and property getters. Called before the generic
/// `lower_native_method_call` path so static factories
/// (`Response.json(v)`) also land here. Returns `Ok(None)` if the
/// (module, method) combination isn't handled.
///
/// Handle ABI note: Response/Headers/Request handles are plain numeric
/// doubles (ids into the runtime's registry), not NaN-boxed pointers.
/// Most runtime functions take the handle as f64; status/statusText/
/// ok/text/json take i64 and we convert via `fptosi`.
pub(in crate::lower_call) fn lower_fetch_native_method(
    ctx: &mut FnCtx<'_>,
    module: &str,
    method: &str,
    object: Option<&Expr>,
    args: &[Expr],
) -> Result<Option<String>> {
    // ── Response static factories (no receiver) ──
    if module == "fetch" && object.is_none() {
        match method {
            "static_json" => {
                // #11789 sweep: `data` is held across the whole `init`
                // evaluation (a callback, a header literal's allocation) and
                // is read back below it.
                let mut json_group = crate::rooting::open_rooted_group(1);
                let v_root = args
                    .first()
                    .map(|data| json_group.lower(ctx, data, args.len() >= 2))
                    .transpose()?;
                // #2638: honor the optional `init` arg
                // (`Response.json(data, { status, statusText, headers })`).
                // Mirror `new Response(body, init)` field extraction: pull
                // `status` (NaN-boxed f64), `statusText` (raw string ptr) and
                // `headers` (a Headers handle, built inline from an object
                // literal) and feed them to the widened runtime helper. Missing
                // fields keep their sentinels (status 200, no statusText, no
                // headers) so the default `Response.json(data)` is unchanged.
                let (status_val, status_text_ptr, headers_handle) = if args.len() >= 2 {
                    lower_response_init(ctx, &args[1])?
                } else {
                    ("200.0".to_string(), "0".to_string(), "0.0".to_string())
                };
                let v = match v_root {
                    Some(root) => json_group.reread(ctx, root)?,
                    None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
                };
                let handle = ctx.block().call(
                    DOUBLE,
                    "js_response_static_json",
                    &[
                        (DOUBLE, &v),
                        (DOUBLE, &status_val),
                        (I64, &status_text_ptr),
                        (DOUBLE, &headers_handle),
                    ],
                );
                json_group.release(ctx);
                return Ok(Some(handle));
            }
            "static_redirect" => {
                let url_ptr = if let Some(url_expr) = args.first() {
                    let url_value = lower_expr(ctx, url_expr)?;
                    ctx.block()
                        .call(I64, "js_jsvalue_to_string", &[(DOUBLE, &url_value)])
                } else {
                    "0".to_string()
                };
                let (arg_values, arg_group) =
                    crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let status = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    "302.0".to_string()
                };
                let handle = ctx.block().call(
                    DOUBLE,
                    "js_response_static_redirect",
                    &[(I64, &url_ptr), (DOUBLE, &status)],
                );
                let result = Some(handle);
                arg_group.release(ctx);
                return Ok(result);
            }
            "static_error" => {
                let handle = ctx.block().call(DOUBLE, "js_response_static_error", &[]);
                return Ok(Some(handle));
            }
            _ => {}
        }
    }

    // Web Streams static factories.
    if module == "readable_stream" && object.is_none() && method == "from" {
        let iterable = if !args.is_empty() {
            lower_expr(ctx, &args[0])?
        } else {
            double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
        };
        let handle = ctx.block().call(
            DOUBLE,
            "js_readable_stream_from_iterable",
            &[(DOUBLE, &iterable)],
        );
        return Ok(Some(handle));
    }

    // Everything below needs a receiver.
    let Some(recv) = object else {
        return Ok(None);
    };

    // ── Headers method dispatch ──
    if module == "Headers" {
        let h_handle = lower_expr(ctx, recv)?;
        match method {
            "set" | "append" => {
                if args.len() < 2 {
                    return Ok(Some(double_literal(0.0)));
                }
                let (arg_values, arg_group) =
                    crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let key_ptr = crate::lower_call::raw_string_ptr_of(ctx, &arg_values[0]);
                let val_ptr = crate::lower_call::raw_string_ptr_of(ctx, &arg_values[1]);
                let runtime_fn = if method == "append" {
                    "js_headers_append"
                } else {
                    "js_headers_set"
                };
                ctx.block().call(
                    DOUBLE,
                    runtime_fn,
                    &[(DOUBLE, &h_handle), (I64, &key_ptr), (I64, &val_ptr)],
                );
                let result = Some(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                arg_group.release(ctx);
                return Ok(result);
            }
            "get" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(0.0)));
                }
                let key_ptr = get_raw_string_ptr(ctx, &args[0])?;
                let str_ptr = ctx.block().call(
                    I64,
                    "js_headers_get",
                    &[(DOUBLE, &h_handle), (I64, &key_ptr)],
                );
                // `js_headers_get` returns a NULL string pointer for an absent
                // header. Per WHATWG `Headers.get` must yield `null` — NaN-boxing
                // the null pointer as a STRING made `typeof h.get(x) === "string"`
                // and `h.get(x) !== null`, so `h.get("x-forwarded-host") ??
                // h.get("host")` never fell through (Auth.js v5's trustHost URL
                // builder then did `new URL("://")` → "Invalid URL", 500 on every
                // authenticated page). Mirror `UrlSearchParamsGet`: box null as
                // TAG_NULL. (Same pattern as searchParams.get.)
                let blk = ctx.block();
                let is_null = blk.icmp_eq(I64, &str_ptr, "0");
                let as_string = nanbox_string_inline(blk, &str_ptr);
                let str_bits = ctx.block().bitcast_double_to_i64(&as_string);
                let selected =
                    ctx.block()
                        .select(I1, &is_null, I64, crate::nanbox::TAG_NULL_I64, &str_bits);
                return Ok(Some(ctx.block().bitcast_i64_to_double(&selected)));
            }
            "getSetCookie" => {
                let arr =
                    ctx.block()
                        .call(DOUBLE, "js_headers_get_set_cookie", &[(DOUBLE, &h_handle)]);
                return Ok(Some(arr));
            }
            "has" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(f64::from_bits(
                        crate::nanbox::TAG_FALSE,
                    ))));
                }
                let key_ptr = get_raw_string_ptr(ctx, &args[0])?;
                let out = ctx.block().call(
                    DOUBLE,
                    "js_headers_has",
                    &[(DOUBLE, &h_handle), (I64, &key_ptr)],
                );
                return Ok(Some(out));
            }
            "delete" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(f64::from_bits(
                        crate::nanbox::TAG_UNDEFINED,
                    ))));
                }
                let key_ptr = get_raw_string_ptr(ctx, &args[0])?;
                ctx.block().call(
                    DOUBLE,
                    "js_headers_delete",
                    &[(DOUBLE, &h_handle), (I64, &key_ptr)],
                );
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            "forEach" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(0.0)));
                }
                let cb = lower_expr(ctx, &args[0])?;
                ctx.block().call(
                    DOUBLE,
                    "js_headers_for_each",
                    &[(DOUBLE, &h_handle), (DOUBLE, &cb)],
                );
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            // `headers.keys()` / `.values()` / `.entries()` return arrays
            // sorted by header name (WHATWG Fetch spec). The arrays are
            // themselves iterable via the array Symbol.iterator, so
            // `for…of`, spread, and `Array.from` all work for free
            // (refs #576).
            "keys" => {
                let arr = ctx
                    .block()
                    .call(DOUBLE, "js_headers_keys", &[(DOUBLE, &h_handle)]);
                return Ok(Some(arr));
            }
            "values" => {
                let arr = ctx
                    .block()
                    .call(DOUBLE, "js_headers_values", &[(DOUBLE, &h_handle)]);
                return Ok(Some(arr));
            }
            "entries" => {
                let arr = ctx
                    .block()
                    .call(DOUBLE, "js_headers_entries", &[(DOUBLE, &h_handle)]);
                return Ok(Some(arr));
            }
            _ => return Ok(None),
        }
    }

    // ── Request property getters ──
    if module == "Request" {
        let h_value = lower_expr(ctx, recv)?;
        let h_handle = ctx
            .block()
            .call(DOUBLE, "js_fetch_unwrap_handle", &[(DOUBLE, &h_value)]);
        match method {
            "url" => {
                let str_ptr = ctx
                    .block()
                    .call(I64, "js_request_get_url", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "method" => {
                let str_ptr =
                    ctx.block()
                        .call(I64, "js_request_get_method", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "destination" => {
                let str_ptr =
                    ctx.block()
                        .call(I64, "js_request_get_destination", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "referrer" => {
                let str_ptr =
                    ctx.block()
                        .call(I64, "js_request_get_referrer", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "referrerPolicy" => {
                let str_ptr = ctx.block().call(
                    I64,
                    "js_request_get_referrer_policy",
                    &[(DOUBLE, &h_handle)],
                );
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "mode" => {
                let str_ptr = ctx
                    .block()
                    .call(I64, "js_request_get_mode", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "credentials" => {
                let str_ptr =
                    ctx.block()
                        .call(I64, "js_request_get_credentials", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "cache" => {
                let str_ptr = ctx
                    .block()
                    .call(I64, "js_request_get_cache", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "redirect" => {
                let str_ptr =
                    ctx.block()
                        .call(I64, "js_request_get_redirect", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "integrity" => {
                let str_ptr =
                    ctx.block()
                        .call(I64, "js_request_get_integrity", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "keepalive" => {
                let out =
                    ctx.block()
                        .call(DOUBLE, "js_request_get_keepalive", &[(DOUBLE, &h_handle)]);
                return Ok(Some(out));
            }
            "duplex" => {
                let str_ptr =
                    ctx.block()
                        .call(I64, "js_request_get_duplex", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "signal" => {
                let out = ctx
                    .block()
                    .call(DOUBLE, "js_request_get_signal", &[(DOUBLE, &h_handle)]);
                return Ok(Some(out));
            }
            "body" => {
                let val = ctx
                    .block()
                    .call(DOUBLE, "js_request_get_body", &[(DOUBLE, &h_handle)]);
                return Ok(Some(val));
            }
            "bodyUsed" => {
                let out = ctx
                    .block()
                    .call(DOUBLE, "js_request_body_used", &[(DOUBLE, &h_handle)]);
                return Ok(Some(out));
            }
            // #1649: `req.headers` returns a `Headers` object (NaN-boxed
            // handle), not the raw numeric request handle. Without this the
            // typed path fell through to `Ok(None)` → the receiver handle
            // surfaced as a number and `req.headers.get(...)` threw
            // "(number).get is not a function", crashing every Hono adapter.
            "headers" => {
                let out =
                    ctx.block()
                        .call(DOUBLE, "js_request_get_headers", &[(DOUBLE, &h_handle)]);
                return Ok(Some(out));
            }
            // #1688: body-consuming methods. `js_request_get_body` already
            // stores the body string; mirror the Response `.text()`/`.json()`/
            // `.arrayBuffer()` path (each returns a Promise pointer NaN-boxed
            // as POINTER_TAG).
            "text" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_request_text", &[(DOUBLE, &h_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "json" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_request_json", &[(DOUBLE, &h_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "arrayBuffer" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_request_array_buffer", &[(DOUBLE, &h_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "blob" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_request_blob", &[(DOUBLE, &h_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "bytes" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_request_bytes", &[(DOUBLE, &h_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "formData" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_request_form_data", &[(DOUBLE, &h_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "clone" => {
                let out = ctx
                    .block()
                    .call(DOUBLE, "js_request_clone", &[(DOUBLE, &h_handle)]);
                return Ok(Some(out));
            }
            _ => return Ok(None),
        }
    }

    // ── Response methods / property getters ──
    if module == "fetch" {
        // Lower the receiver once. It's a NaN-boxed POINTER_TAG handle (Phase 1
        // of the handle-NaN-boxing unification, refs #421) — accessors unbox
        // via `handle_id` on entry, so codegen passes recv_handle through as
        // DOUBLE without any fptosi/bitcast conversion. May also be a chained
        // result from `.headers` / `.clone()` — those cases are recognised at
        // the Call callsite in lower_call.
        let recv_value = lower_expr(ctx, recv)?;
        let recv_handle =
            ctx.block()
                .call(DOUBLE, "js_fetch_unwrap_handle", &[(DOUBLE, &recv_value)]);
        match method {
            "text" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_fetch_response_text", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "json" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_fetch_response_json", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "status" => {
                let blk = ctx.block();
                let status = blk.call(
                    DOUBLE,
                    "js_fetch_response_status",
                    &[(DOUBLE, &recv_handle)],
                );
                return Ok(Some(status));
            }
            "statusText" => {
                let blk = ctx.block();
                let str_ptr = blk.call(
                    I64,
                    "js_fetch_response_status_text",
                    &[(DOUBLE, &recv_handle)],
                );
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "ok" => {
                // js_fetch_response_ok returns 1.0 or 0.0 as f64. Map to
                // TAG_TRUE/TAG_FALSE so console.log prints "true"/"false".
                let blk = ctx.block();
                let raw = blk.call(DOUBLE, "js_fetch_response_ok", &[(DOUBLE, &recv_handle)]);
                let cmp = blk.fcmp("une", &raw, "0.0");
                let tagged = blk.select(
                    crate::types::I1,
                    &cmp,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(Some(blk.bitcast_i64_to_double(&tagged)));
            }
            "type" => {
                let blk = ctx.block();
                let str_ptr = blk.call(I64, "js_fetch_response_type", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "url" => {
                let blk = ctx.block();
                let str_ptr = blk.call(I64, "js_fetch_response_url", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "redirected" => {
                let out = ctx.block().call(
                    DOUBLE,
                    "js_fetch_response_redirected",
                    &[(DOUBLE, &recv_handle)],
                );
                return Ok(Some(out));
            }
            "bodyUsed" => {
                let out =
                    ctx.block()
                        .call(DOUBLE, "js_response_body_used", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(out));
            }
            "headers" => {
                let out =
                    ctx.block()
                        .call(DOUBLE, "js_response_get_headers", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(out));
            }
            "clone" => {
                let out = ctx
                    .block()
                    .call(DOUBLE, "js_response_clone", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(out));
            }
            "arrayBuffer" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_response_array_buffer", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "blob" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_response_blob", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "bytes" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_response_bytes", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "formData" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_response_form_data", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            // Issue #237: response.body — returns ReadableStream over the
            // buffered body bytes. Property access lowers as a zero-arg
            // method call here, same as response.headers above.
            "body" => {
                let h = ctx
                    .block()
                    .call(DOUBLE, "js_response_body", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(h));
            }
            _ => return Ok(None),
        }
    }

    if module == "FormData" {
        let handle = lower_expr(ctx, recv)?;
        match method {
            "append" | "set" => {
                let (arg_values, arg_group) =
                    crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let name = if !args.is_empty() {
                    arg_values[0].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let value = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let filename = if args.len() >= 3 {
                    arg_values[2].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let runtime_fn = if method == "append" {
                    "js_form_data_append"
                } else {
                    "js_form_data_set"
                };
                ctx.block().call(
                    DOUBLE,
                    runtime_fn,
                    &[
                        (DOUBLE, &handle),
                        (DOUBLE, &name),
                        (DOUBLE, &value),
                        (DOUBLE, &filename),
                    ],
                );
                let result = Some(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                arg_group.release(ctx);
                return Ok(result);
            }
            "delete" => {
                let key_ptr = if args.is_empty() {
                    "0".to_string()
                } else {
                    get_raw_string_ptr(ctx, &args[0])?
                };
                ctx.block().call(
                    DOUBLE,
                    "js_form_data_delete",
                    &[(DOUBLE, &handle), (I64, &key_ptr)],
                );
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            "get" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(f64::from_bits(
                        crate::nanbox::TAG_NULL,
                    ))));
                }
                let key_ptr = get_raw_string_ptr(ctx, &args[0])?;
                let value = ctx.block().call(
                    DOUBLE,
                    "js_form_data_get",
                    &[(DOUBLE, &handle), (I64, &key_ptr)],
                );
                return Ok(Some(value));
            }
            "has" => {
                let key_ptr = if args.is_empty() {
                    "0".to_string()
                } else {
                    get_raw_string_ptr(ctx, &args[0])?
                };
                let value = ctx.block().call(
                    DOUBLE,
                    "js_form_data_has",
                    &[(DOUBLE, &handle), (I64, &key_ptr)],
                );
                return Ok(Some(value));
            }
            "getAll" => {
                let key_ptr = if args.is_empty() {
                    "0".to_string()
                } else {
                    get_raw_string_ptr(ctx, &args[0])?
                };
                let arr = ctx.block().call(
                    DOUBLE,
                    "js_form_data_get_all",
                    &[(DOUBLE, &handle), (I64, &key_ptr)],
                );
                return Ok(Some(arr));
            }
            "entries" => {
                let arr = ctx
                    .block()
                    .call(DOUBLE, "js_form_data_entries", &[(DOUBLE, &handle)]);
                return Ok(Some(arr));
            }
            "keys" => {
                let arr = ctx
                    .block()
                    .call(DOUBLE, "js_form_data_keys", &[(DOUBLE, &handle)]);
                return Ok(Some(arr));
            }
            "values" => {
                let arr = ctx
                    .block()
                    .call(DOUBLE, "js_form_data_values", &[(DOUBLE, &handle)]);
                return Ok(Some(arr));
            }
            "forEach" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(f64::from_bits(
                        crate::nanbox::TAG_UNDEFINED,
                    ))));
                }
                let cb = lower_expr(ctx, &args[0])?;
                ctx.block().call(
                    DOUBLE,
                    "js_form_data_for_each",
                    &[(DOUBLE, &handle), (DOUBLE, &cb)],
                );
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            _ => return Ok(None),
        }
    }

    // ── Blob instance methods + property getters (issue #234) ──
    // The receiver is a numeric Blob handle (registry id) carried as f64,
    // mirroring the Response handle ABI. Locals are tagged blob::Blob via
    // `register_native_instance` in `destructuring.rs`.
    if module == "blob" {
        let recv_handle = lower_expr(ctx, recv)?;
        match method {
            "size" => {
                let blk = ctx.block();
                let n = blk.call(DOUBLE, "js_blob_size", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(n));
            }
            "type" => {
                let str_ptr = ctx
                    .block()
                    .call(I64, "js_blob_type", &[(DOUBLE, &recv_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "arrayBuffer" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_blob_array_buffer", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "bytes" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_blob_bytes", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "text" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_blob_text", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "slice" => {
                // slice(start?, end?, type?) — missing numeric args use
                // canonical f64::NAN as sentinel; missing type uses null
                // pointer (0). Runtime `js_blob_slice` checks `is_nan()`
                // / `type_ptr.is_null()` to apply WHATWG defaults
                // (start=0, end=len, type="").
                let (arg_values, arg_group) =
                    crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let start = if !args.is_empty() {
                    arg_values[0].clone()
                } else {
                    double_literal(f64::NAN)
                };
                let end = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::NAN)
                };
                let type_ptr = if args.len() >= 3 {
                    crate::lower_call::raw_string_ptr_of(ctx, &arg_values[2])
                } else {
                    "0".to_string()
                };
                let new_handle = ctx.block().call(
                    DOUBLE,
                    "js_blob_slice",
                    &[
                        (DOUBLE, &recv_handle),
                        (DOUBLE, &start),
                        (DOUBLE, &end),
                        (I64, &type_ptr),
                    ],
                );
                let result = Some(new_handle);
                arg_group.release(ctx);
                return Ok(result);
            }
            // Issue #237: blob.stream() — returns ReadableStream over the
            // blob's bytes. Single-chunk; closes after one read.
            "stream" => {
                let h = ctx
                    .block()
                    .call(DOUBLE, "js_blob_stream", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(h));
            }
            // Issue #1211: File-specific properties.  Plain Blob handles
            // resolve `name` to the empty string and `lastModified` to 0,
            // which matches Node's behavior for non-File Blobs (no
            // ambiguity from sharing the registry).
            "name" => {
                let str_ptr = ctx
                    .block()
                    .call(I64, "js_file_name", &[(DOUBLE, &recv_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "lastModified" => {
                let blk = ctx.block();
                let n = blk.call(DOUBLE, "js_file_last_modified", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(n));
            }
            _ => return Ok(None),
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // Web Streams API (issue #237)
    // The receivers are numeric registry-id handles carried as f64,
    // mirroring the Blob/Response handle ABI. Locals are tagged
    // (module, class_name) by `register_native_instance` in
    // `destructuring.rs`.
    // ─────────────────────────────────────────────────────────────────

    if module == "readable_stream" {
        let recv_handle_raw = lower_expr(ctx, recv)?;
        // Issue #562: subclass instances stash the handle id under
        // `__perry_stream_handle__`; bare numeric handles pass through
        // unchanged. Cheap (one runtime call) and applied uniformly so
        // the FFIs below see a clean registry id either way.
        let recv_handle = ctx.block().call(
            DOUBLE,
            "js_stream_unwrap_handle",
            &[(DOUBLE, &recv_handle_raw)],
        );
        match method {
            "getReader" => {
                let options = if !args.is_empty() {
                    lower_expr(ctx, &args[0])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let h = ctx.block().call(
                    DOUBLE,
                    "js_readable_stream_get_reader_with_options",
                    &[(DOUBLE, &recv_handle), (DOUBLE, &options)],
                );
                return Ok(Some(h));
            }
            "cancel" => {
                let reason = if !args.is_empty() {
                    lower_expr(ctx, &args[0])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let blk = ctx.block();
                let promise = blk.call(
                    I64,
                    "js_readable_stream_cancel",
                    &[(DOUBLE, &recv_handle), (DOUBLE, &reason)],
                );
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "tee" => {
                let h =
                    ctx.block()
                        .call(DOUBLE, "js_readable_stream_tee", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(h));
            }
            "pipeTo" => {
                let (arg_values, arg_group) =
                    crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let dest_raw = if !args.is_empty() {
                    arg_values[0].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let options = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                // Issue #562: `dest` may be a subclass instance — unwrap.
                let dest =
                    ctx.block()
                        .call(DOUBLE, "js_stream_unwrap_handle", &[(DOUBLE, &dest_raw)]);
                let blk = ctx.block();
                let promise = blk.call(
                    I64,
                    "js_readable_stream_pipe_to",
                    &[(DOUBLE, &recv_handle), (DOUBLE, &dest), (DOUBLE, &options)],
                );
                let result = Some(nanbox_pointer_inline(blk, &promise));
                arg_group.release(ctx);
                return Ok(result);
            }
            "pipeThrough" => {
                // Preserve the original numeric sequence for a write-stable
                // native TransformStream and no options. This proof comes from
                // its initializer's runtime contract, never a TS annotation.
                let numeric_transform = args.len() == 1
                    && !ctx.classes.contains_key("TransformStream")
                    && !(ctx.import_function_prefixes.contains_key("TransformStream")
                        && !ctx
                            .import_function_v8_specifiers
                            .contains_key("TransformStream"))
                    && matches!(
                        crate::type_analysis::proven_type_from_init(ctx, &args[0]),
                        Some(perry_hir::types::Type::Named(name)) if name == "TransformStream"
                    );
                if numeric_transform {
                    // Only this path takes the argument from a group; the pair
                    // path below owns its own rooting and must not see the
                    // arguments lowered twice (`options()` ran twice).
                    let (arg_values, arg_group) =
                        crate::lower_call::lower_call_args_rooted(ctx, &args[..1])?;
                    let transform_raw = arg_values[0].clone();
                    let transform = ctx.block().call(
                        DOUBLE,
                        "js_stream_unwrap_handle",
                        &[(DOUBLE, &transform_raw)],
                    );
                    let writable = ctx.block().call(
                        DOUBLE,
                        "js_transform_stream_writable",
                        &[(DOUBLE, &transform)],
                    );
                    let readable = ctx.block().call(
                        DOUBLE,
                        "js_transform_stream_readable",
                        &[(DOUBLE, &transform)],
                    );
                    let options = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    let _ = ctx.block().call(
                        DOUBLE,
                        "js_readable_stream_pipe_through_validate",
                        &[
                            (DOUBLE, &recv_handle),
                            (DOUBLE, &writable),
                            (DOUBLE, &readable),
                            (DOUBLE, &options),
                        ],
                    );
                    let pipe = ctx.block().call(
                        I64,
                        "js_readable_stream_pipe_to",
                        &[
                            (DOUBLE, &recv_handle),
                            (DOUBLE, &writable),
                            (DOUBLE, &options),
                        ],
                    );
                    ctx.block()
                        .call_void("js_promise_mark_internally_handled", &[(I64, &pipe)]);
                    let result = Some(readable);
                    arg_group.release(ctx);
                    return Ok(result);
                }
                // Evaluate both arguments before the pair getters. The runtime
                // owns roots across getters; only options evaluation can move
                // the transform before the call. With no options, no temp root
                // is emitted (including the numeric TransformStream path).
                let output = with_rooted_group(ctx, 2, |ctx, roots| {
                    let transform = if let Some(expr) = args.first() {
                        let collects = any_operand_may_collect(ctx, args[1..].iter());
                        Some(roots.lower(ctx, expr, collects)?)
                    } else {
                        None
                    };
                    let options = if let Some(expr) = args.get(1) {
                        Some(roots.lower(ctx, expr, false)?)
                    } else {
                        None
                    };
                    let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    let transform = match transform {
                        Some(i) => roots.reread(ctx, i)?,
                        None => undefined.clone(),
                    };
                    let options = match options {
                        Some(i) => roots.reread(ctx, i)?,
                        None => undefined,
                    };
                    Ok(ctx.block().call(
                        DOUBLE,
                        "js_readable_stream_pipe_through_pair",
                        &[
                            (DOUBLE, &recv_handle),
                            (DOUBLE, &transform),
                            (DOUBLE, &options),
                        ],
                    ))
                })?;
                return Ok(Some(output));
            }
            "locked" => {
                let v = ctx.block().call(
                    DOUBLE,
                    "js_readable_stream_locked",
                    &[(DOUBLE, &recv_handle)],
                );
                return Ok(Some(v));
            }
            // ReadableStreamDefaultController on the same handle:
            "enqueue" => {
                let chunk = if !args.is_empty() {
                    lower_expr(ctx, &args[0])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let v = ctx.block().call(
                    DOUBLE,
                    "js_readable_stream_controller_enqueue",
                    &[(DOUBLE, &recv_handle), (DOUBLE, &chunk)],
                );
                return Ok(Some(v));
            }
            "close" => {
                let v = ctx.block().call(
                    DOUBLE,
                    "js_readable_stream_controller_close",
                    &[(DOUBLE, &recv_handle)],
                );
                return Ok(Some(v));
            }
            "error" => {
                let reason = if !args.is_empty() {
                    lower_expr(ctx, &args[0])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let v = ctx.block().call(
                    DOUBLE,
                    "js_readable_stream_controller_error",
                    &[(DOUBLE, &recv_handle), (DOUBLE, &reason)],
                );
                return Ok(Some(v));
            }
            "desiredSize" => {
                let v = ctx.block().call(
                    DOUBLE,
                    "js_readable_stream_controller_desired_size",
                    &[(DOUBLE, &recv_handle)],
                );
                return Ok(Some(v));
            }
            // #4915: byte-stream controller's BYOB request — `{ view,
            // respond, respondWithNewView }` while a BYOB read is parked,
            // null otherwise.
            "byobRequest" => {
                let v = ctx.block().call(
                    DOUBLE,
                    "js_readable_stream_controller_byob_request",
                    &[(DOUBLE, &recv_handle)],
                );
                return Ok(Some(v));
            }
            _ => return Ok(None),
        }
    }

    if module == "readable_stream_reader" {
        let recv_handle = lower_expr(ctx, recv)?;
        match method {
            // #4915: `read(view)` is the BYOB form — the runtime fills the
            // caller-supplied view (default readers ignore the argument).
            "read" if !args.is_empty() => {
                let view = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let promise = blk.call(
                    I64,
                    "js_reader_read_with_view",
                    &[(DOUBLE, &recv_handle), (DOUBLE, &view)],
                );
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "read" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_reader_read", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "releaseLock" => {
                let v =
                    ctx.block()
                        .call(DOUBLE, "js_reader_release_lock", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(v));
            }
            "cancel" => {
                let reason = if !args.is_empty() {
                    lower_expr(ctx, &args[0])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let blk = ctx.block();
                let promise = blk.call(
                    I64,
                    "js_reader_cancel",
                    &[(DOUBLE, &recv_handle), (DOUBLE, &reason)],
                );
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "closed" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_reader_closed", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            _ => return Ok(None),
        }
    }

    if module == "writable_stream" {
        let recv_handle_raw = lower_expr(ctx, recv)?;
        // Issue #562: subclass instances unwrap to a numeric handle.
        let recv_handle = ctx.block().call(
            DOUBLE,
            "js_stream_unwrap_handle",
            &[(DOUBLE, &recv_handle_raw)],
        );
        match method {
            "getWriter" => {
                let h = ctx.block().call(
                    DOUBLE,
                    "js_writable_stream_get_writer",
                    &[(DOUBLE, &recv_handle)],
                );
                return Ok(Some(h));
            }
            "abort" => {
                let reason = if !args.is_empty() {
                    lower_expr(ctx, &args[0])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let blk = ctx.block();
                let promise = blk.call(
                    I64,
                    "js_writable_stream_abort",
                    &[(DOUBLE, &recv_handle), (DOUBLE, &reason)],
                );
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "close" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_writable_stream_close", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "locked" => {
                let v = ctx.block().call(
                    DOUBLE,
                    "js_writable_stream_locked",
                    &[(DOUBLE, &recv_handle)],
                );
                return Ok(Some(v));
            }
            _ => return Ok(None),
        }
    }

    if module == "writable_stream_writer" {
        let recv_handle = lower_expr(ctx, recv)?;
        match method {
            "write" => {
                let chunk = if !args.is_empty() {
                    lower_expr(ctx, &args[0])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let blk = ctx.block();
                let promise = blk.call(
                    I64,
                    "js_writer_write",
                    &[(DOUBLE, &recv_handle), (DOUBLE, &chunk)],
                );
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "close" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_writer_close", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "abort" => {
                let reason = if !args.is_empty() {
                    lower_expr(ctx, &args[0])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let blk = ctx.block();
                let promise = blk.call(
                    I64,
                    "js_writer_abort",
                    &[(DOUBLE, &recv_handle), (DOUBLE, &reason)],
                );
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "releaseLock" => {
                let v =
                    ctx.block()
                        .call(DOUBLE, "js_writer_release_lock", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(v));
            }
            "closed" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_writer_closed", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "ready" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_writer_ready", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "desiredSize" => {
                let v =
                    ctx.block()
                        .call(DOUBLE, "js_writer_desired_size", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(v));
            }
            _ => return Ok(None),
        }
    }

    if module == "transform_stream" {
        let recv_handle_raw = lower_expr(ctx, recv)?;
        // Issue #562: subclass instances unwrap to a numeric handle.
        let recv_handle = ctx.block().call(
            DOUBLE,
            "js_stream_unwrap_handle",
            &[(DOUBLE, &recv_handle_raw)],
        );
        match method {
            "readable" => {
                let v = ctx.block().call(
                    DOUBLE,
                    "js_transform_stream_readable",
                    &[(DOUBLE, &recv_handle)],
                );
                return Ok(Some(v));
            }
            "writable" => {
                let v = ctx.block().call(
                    DOUBLE,
                    "js_transform_stream_writable",
                    &[(DOUBLE, &recv_handle)],
                );
                return Ok(Some(v));
            }
            _ => return Ok(None),
        }
    }

    Ok(None)
}

/// Lower a `Response` init (`{ status?, statusText?, headers? }`, or any
/// runtime value standing for one) to the three values `js_response_new` /
/// `js_response_static_json` take: the NaN-boxed status, the RAW `statusText`
/// string pointer, and the Headers handle.
///
/// Shared by `Response.json(data, init)` and `new Response(body, init)`, which
/// had two copies of this and both lowered each field into a bare register.
/// #11789 sweep: every field value is lowered and rooted FIRST, in source
/// order, each across the ones after it (a header literal builds an
/// allocating Headers handle, and a field expression is arbitrary user code);
/// the raw `statusText` pointer is taken from the re-read, so nothing
/// collects between it and the call that consumes it. A Headers handle is a
/// plain registry id, not a heap reference, so one built from a literal
/// needs no root.
pub(in crate::lower_call) fn lower_response_init(
    ctx: &mut FnCtx<'_>,
    init: &Expr,
) -> Result<(String, String, String)> {
    let mut status_val = "200.0".to_string();
    let mut status_text_ptr = "0".to_string();
    let mut headers_handle = "0.0".to_string();
    if let Some(props) = super::extract_options_fields(ctx, init) {
        enum Slot {
            Status(usize),
            StatusText(usize),
            Headers(usize),
            HeadersBuilt(String),
            Discard,
        }
        let mut group = crate::rooting::open_rooted_group(props.len());
        let mut slots: Vec<Slot> = Vec::with_capacity(props.len());
        for (i, (k, vexpr)) in props.iter().enumerate() {
            let collects = props[i + 1..].iter().any(|(later_key, later)| {
                later_key == "headers" || any_operand_may_collect(ctx, std::iter::once(later))
            });
            match k.as_str() {
                "status" => slots.push(Slot::Status(group.lower(ctx, vexpr, collects)?)),
                "statusText" => slots.push(Slot::StatusText(group.lower(ctx, vexpr, collects)?)),
                "headers" => {
                    // Inline object → build a Headers handle.
                    // Phase 3 anon-class → same via extract_options.
                    // Other expressions → use as-is (handle f64).
                    if let Some(hprops) = super::extract_options_fields(ctx, vexpr) {
                        slots.push(Slot::HeadersBuilt(super::build_headers_from_object(
                            ctx, &hprops,
                        )?));
                    } else {
                        slots.push(Slot::Headers(group.lower(ctx, vexpr, collects)?));
                    }
                }
                _ => {
                    group.lower(ctx, vexpr, collects)?;
                    slots.push(Slot::Discard);
                }
            }
        }
        for slot in slots {
            match slot {
                Slot::Status(root) => status_val = group.reread(ctx, root)?,
                Slot::StatusText(root) => {
                    let text = group.reread(ctx, root)?;
                    status_text_ptr = super::raw_string_ptr_of(ctx, &text);
                }
                Slot::Headers(root) => headers_handle = group.reread(ctx, root)?,
                Slot::HeadersBuilt(handle) => headers_handle = handle,
                Slot::Discard => {}
            }
        }
        group.release(ctx);
    } else {
        // The init is a RUNTIME object, not a literal -- a bound variable
        // (`Response.json(data, init)` where `init` is a param or local, or
        // another `Response`). `extract_options_fields` only sees object
        // literals, so the whole init was dropped and the status defaulted to
        // 200: `Response.json(x, {status:401})` returned 401 at module scope
        // (literal) but 200 the moment the init was passed through a variable
        // -- e.g. inside `NextResponse.json`, so every authenticated route's
        // 401 became a 200. Mirror the `new Response(body, init)` runtime path
        // and read the fields at runtime.
        //
        // `init` is a runtime value that need not be an object:
        // `Response.json(x, 3.14)` is legal TS. Unboxing it to a raw pointer
        // and dereferencing would SIGSEGV (a non-integer double's bits land in
        // the heap-pointer magnitude window). Read each field through the
        // boxed helper, which validates the receiver and returns `undefined`
        // for a non-object instead of derefing.
        let mut group = crate::rooting::open_rooted_group(3);
        let opts_root = group.lower(ctx, init, true)?;
        let get_field = |ctx_inner: &mut FnCtx<'_>, opts_val: &str, key: &str| -> String {
            let key_idx = ctx_inner.strings.intern(key);
            let key_global = format!("@{}", ctx_inner.strings.entry(key_idx).handle_global);
            let blk = ctx_inner.block();
            let key_box = blk.load(DOUBLE, &key_global);
            let key_bits = blk.bitcast_double_to_i64(&key_box);
            let key_raw = blk.and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
            blk.call(
                DOUBLE,
                "js_object_get_field_by_name_boxed",
                &[(DOUBLE, opts_val), (I64, &key_raw)],
            )
        };
        // Each field read can run a getter, so the values already read and the
        // options object itself stay rooted across the reads after them.
        let opts = group.reread(ctx, opts_root)?;
        let status_box = get_field(ctx, &opts, "status");
        let status_root = group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &status_box, true);
        let opts = group.reread(ctx, opts_root)?;
        let st_box = get_field(ctx, &opts, "statusText");
        let st_root = group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &st_box, true);
        let opts = group.reread(ctx, opts_root)?;
        headers_handle = get_field(ctx, &opts, "headers");
        let headers_root =
            group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &headers_handle, true);
        status_val = group.reread_emitted(ctx, status_root);
        let st_box = group.reread_emitted(ctx, st_root);
        headers_handle = group.reread_emitted(ctx, headers_root);
        status_text_ptr = super::raw_string_ptr_of(ctx, &st_box);
        group.release(ctx);
    }
    Ok((status_val, status_text_ptr, headers_handle))
}
