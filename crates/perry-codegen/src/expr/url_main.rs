//! URL / URLSearchParams + FsRmRecursive.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, bail, Result};
#[allow(unused_imports)]
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp};
#[allow(unused_imports)]
use perry_types::Type as HirType;

#[allow(unused_imports)]
use crate::lower_call::{lower_call, lower_native_method_call, lower_new};
#[allow(unused_imports)]
use crate::lower_conditional::{lower_conditional, lower_logical, lower_truthy};
#[allow(unused_imports)]
use crate::lower_string_method::{
    flatten_string_add_chain, lower_string_coerce_concat, lower_string_concat,
    lower_string_concat_chain, lower_string_self_append,
};
#[allow(unused_imports)]
use crate::nanbox::{double_literal, POINTER_MASK_I64};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_alias_metadata_suffix, can_lower_expr_as_i32, emit_layout_note_slot_on_block,
    emit_shadow_slot_clear, emit_shadow_slot_update_for_expr, emit_string_literal_global,
    emit_v8_export_call, emit_v8_member_method_call, emit_write_barrier,
    emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, try_flat_const_2d_int,
    try_lower_flat_const_index_get, try_match_channel_reduction, try_static_class_name,
    unbox_str_handle, unbox_to_i64, variant_name, ChannelReduction, FlatConstInfo, FnCtx,
    I18nLowerCtx,
};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::FileURLToPath(url) => {
            let v = lower_expr(ctx, url)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_url_file_url_to_path", &[(DOUBLE, &v)]))
        }

        Expr::UrlNew { url, base } => {
            // #3055: `new URL(input[, base])` applies `String(value)` coercion
            // to both arguments (numbers/null/objects stringify, Symbols throw)
            // BEFORE parsing. `js_url_coerce_string` replaces plain
            // string-pointer extraction, which dropped non-string values to a
            // null/garbage pointer.
            let url_v = lower_expr(ctx, url)?;
            let url_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &url_v)]);
            let obj = if let Some(base) = base {
                let base_v = lower_expr(ctx, base)?;
                let base_ptr = ctx
                    .block()
                    .call(I64, "js_url_coerce_string", &[(DOUBLE, &base_v)]);
                ctx.block().call(
                    I64,
                    "js_url_new_with_base",
                    &[(I64, &url_ptr), (I64, &base_ptr)],
                )
            } else {
                ctx.block().call(I64, "js_url_new", &[(I64, &url_ptr)])
            };
            Ok(nanbox_pointer_inline(ctx.block(), &obj))
        }

        // The nine scalar URL getters. Runtime returns an already-NaN-boxed
        // f64 string, so no retagging needed.
        Expr::UrlGetHref(u) => lower_url_string_getter(ctx, u, "js_url_get_href"),
        Expr::UrlGetPathname(u) => lower_url_string_getter(ctx, u, "js_url_get_pathname"),
        Expr::UrlGetProtocol(u) => lower_url_string_getter(ctx, u, "js_url_get_protocol"),
        Expr::UrlGetHost(u) => lower_url_string_getter(ctx, u, "js_url_get_host"),
        Expr::UrlGetHostname(u) => lower_url_string_getter(ctx, u, "js_url_get_hostname"),
        Expr::UrlGetPort(u) => lower_url_string_getter(ctx, u, "js_url_get_port"),
        Expr::UrlGetSearch(u) => lower_url_string_getter(ctx, u, "js_url_get_search"),
        Expr::UrlGetHash(u) => lower_url_string_getter(ctx, u, "js_url_get_hash"),
        Expr::UrlGetOrigin(u) => lower_url_string_getter(ctx, u, "js_url_get_origin"),

        Expr::UrlGetSearchParams(u) => {
            // Runtime stores an already-NaN-boxed URLSearchParams pointer in
            // the URL object's `searchParams` field (see create_url_object in
            // perry-runtime/src/url.rs).
            lower_url_string_getter(ctx, u, "js_url_get_search_params")
        }

        // Issue #650: `urlInstance.toString()` and `.toJSON()` both return
        // the URL's href per WHATWG. Reuses `js_url_get_href` since the
        // value is identical.
        Expr::UrlInstanceToString(u) => lower_url_string_getter(ctx, u, "js_url_get_href"),
        Expr::UrlInstanceToJSON(u) => lower_url_string_getter(ctx, u, "js_url_get_href"),

        // Issue #650: URL setters — runtime helper updates the named field
        // AND re-derives `href` so subsequent .href reads see the new
        // composed string. Returns the assigned value (matches JS
        // assignment expression semantics).
        Expr::UrlSetPathname { url, value }
        | Expr::UrlSetSearch { url, value }
        | Expr::UrlSetHash { url, value }
        | Expr::UrlSetProtocol { url, value }
        | Expr::UrlSetHostname { url, value }
        | Expr::UrlSetPort { url, value }
        | Expr::UrlSetUsername { url, value }
        | Expr::UrlSetPassword { url, value }
        | Expr::UrlSetHref { url, value } => {
            let runtime_fn = match expr {
                Expr::UrlSetPathname { .. } => "js_url_set_pathname",
                Expr::UrlSetSearch { .. } => "js_url_set_search",
                Expr::UrlSetHash { .. } => "js_url_set_hash",
                Expr::UrlSetProtocol { .. } => "js_url_set_protocol",
                Expr::UrlSetHostname { .. } => "js_url_set_hostname",
                Expr::UrlSetPort { .. } => "js_url_set_port",
                Expr::UrlSetUsername { .. } => "js_url_set_username",
                Expr::UrlSetPassword { .. } => "js_url_set_password",
                Expr::UrlSetHref { .. } => "js_url_set_href",
                _ => unreachable!(),
            };
            let url_v = lower_expr(ctx, url)?;
            let url_handle = unbox_to_i64(ctx.block(), &url_v);
            let val_v = lower_expr(ctx, value)?;
            let val_str_ptr =
                ctx.block()
                    .call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &val_v)]);
            ctx.block()
                .call_void(runtime_fn, &[(I64, &url_handle), (I64, &val_str_ptr)]);
            // Assignment expression evaluates to the value on the RHS.
            Ok(val_v)
        }

        // Issue #650: URL.canParse(s) -> boolean. Runtime returns 1/0 as i32;
        // we NaN-box to TAG_TRUE / TAG_FALSE to match perry's boolean repr.
        Expr::UrlCanParse(arg) => {
            // #3054: coerce the input via `String(value)` (Symbols throw).
            let v = lower_expr(ctx, arg)?;
            let str_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &v)]);
            let result_i32 = ctx
                .block()
                .call(I32, "js_url_can_parse", &[(I64, &str_ptr)]);
            let blk = ctx.block();
            let is_true = blk.icmp_ne(I32, &result_i32, "0");
            let tagged = blk.select(
                I1,
                &is_true,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        Expr::UrlCanParseWithBase { input, base } => {
            // #3054: coerce input + base via `String(value)` (Symbols throw).
            let input_v = lower_expr(ctx, input)?;
            let input_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &input_v)]);
            let base_v = lower_expr(ctx, base)?;
            let base_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &base_v)]);
            let result_i32 = ctx.block().call(
                I32,
                "js_url_can_parse_with_base",
                &[(I64, &input_ptr), (I64, &base_ptr)],
            );
            let blk = ctx.block();
            let is_true = blk.icmp_ne(I32, &result_i32, "0");
            let tagged = blk.select(
                I1,
                &is_true,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // Issue #650: URL.parse(s) -> URL | null. Runtime returns the same
        // ObjectHeader* `js_url_new` produces on success, or null when the
        // input fails to parse.
        Expr::UrlParse(arg) => {
            // #3054: coerce the input via `String(value)` (Symbols throw).
            let v = lower_expr(ctx, arg)?;
            let str_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &v)]);
            let obj = ctx.block().call(I64, "js_url_parse", &[(I64, &str_ptr)]);
            // Runtime returns 0 for parse failure; we map that to TAG_NULL so
            // `URL.parse(bad)?.href` short-circuits via optional-chain semantics.
            let blk = ctx.block();
            let is_null = blk.icmp_eq(I64, &obj, "0");
            let success = nanbox_pointer_inline(blk, &obj);
            let null_box = blk.bitcast_i64_to_double(crate::nanbox::TAG_NULL_I64);
            let blk = ctx.block();
            Ok(blk.select(I1, &is_null, DOUBLE, &null_box, &success))
        }

        Expr::UrlParseWithBase { input, base } => {
            // #3054: coerce input + base via `String(value)` (Symbols throw).
            let input_v = lower_expr(ctx, input)?;
            let input_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &input_v)]);
            let base_v = lower_expr(ctx, base)?;
            let base_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &base_v)]);
            let obj = ctx.block().call(
                I64,
                "js_url_parse_with_base",
                &[(I64, &input_ptr), (I64, &base_ptr)],
            );
            let blk = ctx.block();
            let is_null = blk.icmp_eq(I64, &obj, "0");
            let success = nanbox_pointer_inline(blk, &obj);
            let null_box = blk.bitcast_i64_to_double(crate::nanbox::TAG_NULL_I64);
            let blk = ctx.block();
            Ok(blk.select(I1, &is_null, DOUBLE, &null_box, &success))
        }

        Expr::UrlSearchParamsNew(init) => {
            // Pre-#575 this routed every init through `js_url_search_params_new`
            // which only accepts a string — object literals (`new
            // URLSearchParams({a:"1"})`) reached here as NaN-boxed pointers
            // and `js_get_string_pointer_unified` re-interpreted the pointer
            // bits as a `*mut StringHeader`, reading garbage. We now hand the
            // init f64 to `js_url_search_params_new_any` which decodes
            // string / record / URLSearchParams / null / undefined at runtime.
            let params_obj = if let Some(init) = init {
                let v = lower_expr(ctx, init)?;
                ctx.block()
                    .call(I64, "js_url_search_params_new_any", &[(DOUBLE, &v)])
            } else {
                ctx.block().call(I64, "js_url_search_params_new_empty", &[])
            };
            Ok(nanbox_pointer_inline(ctx.block(), &params_obj))
        }

        Expr::UrlSearchParamsGet { params, name } => {
            let p_v = lower_expr(ctx, params)?;
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            let n_v = lower_expr(ctx, name)?;
            let n_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &n_v)]);
            let str_ptr = ctx.block().call(
                I64,
                "js_url_search_params_get",
                &[(I64, &p_ptr), (I64, &n_ptr)],
            );
            // Runtime returns a null pointer when the key is absent;
            // JS expects `null` in that case, not an empty string.
            let blk = ctx.block();
            let is_null = blk.icmp_eq(I64, &str_ptr, "0");
            let as_string = nanbox_string_inline(blk, &str_ptr);
            let str_bits = ctx.block().bitcast_double_to_i64(&as_string);
            let selected =
                ctx.block()
                    .select(I1, &is_null, I64, crate::nanbox::TAG_NULL_I64, &str_bits);
            Ok(ctx.block().bitcast_i64_to_double(&selected))
        }

        Expr::UrlSearchParamsHas {
            params,
            name,
            value,
        } => {
            let p_v = lower_expr(ctx, params)?;
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            let n_v = lower_expr(ctx, name)?;
            let n_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &n_v)]);
            // Runtime returns 0.0 / 1.0 as a plain f64 — not NaN-boxed.
            // Translate to TAG_TRUE / TAG_FALSE so `typeof` and strict-eq
            // behave correctly.
            let raw = if let Some(v_expr) = value {
                let v_v = lower_expr(ctx, v_expr)?;
                let v_ptr = ctx
                    .block()
                    .call(I64, "js_url_coerce_string", &[(DOUBLE, &v_v)]);
                ctx.block().call(
                    DOUBLE,
                    "js_url_search_params_has2",
                    &[(I64, &p_ptr), (I64, &n_ptr), (I64, &v_ptr)],
                )
            } else {
                ctx.block().call(
                    DOUBLE,
                    "js_url_search_params_has",
                    &[(I64, &p_ptr), (I64, &n_ptr)],
                )
            };
            let blk = ctx.block();
            let is_true = blk.fcmp("une", &raw, &double_literal(0.0));
            let tagged = blk.select(
                I1,
                &is_true,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(ctx.block().bitcast_i64_to_double(&tagged))
        }

        Expr::UrlSearchParamsSet {
            params,
            name,
            value,
        } => {
            let p_v = lower_expr(ctx, params)?;
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            let n_v = lower_expr(ctx, name)?;
            let n_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &n_v)]);
            let val_v = lower_expr(ctx, value)?;
            let val_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &val_v)]);
            ctx.block().call_void(
                "js_url_search_params_set",
                &[(I64, &p_ptr), (I64, &n_ptr), (I64, &val_ptr)],
            );
            Ok(ctx
                .block()
                .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64))
        }

        Expr::UrlSearchParamsAppend {
            params,
            name,
            value,
        } => {
            let p_v = lower_expr(ctx, params)?;
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            let n_v = lower_expr(ctx, name)?;
            let n_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &n_v)]);
            let val_v = lower_expr(ctx, value)?;
            let val_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &val_v)]);
            ctx.block().call_void(
                "js_url_search_params_append",
                &[(I64, &p_ptr), (I64, &n_ptr), (I64, &val_ptr)],
            );
            Ok(ctx
                .block()
                .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64))
        }

        Expr::UrlSearchParamsDelete {
            params,
            name,
            value,
        } => {
            let p_v = lower_expr(ctx, params)?;
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            let n_v = lower_expr(ctx, name)?;
            let n_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &n_v)]);
            if let Some(v_expr) = value {
                let v_v = lower_expr(ctx, v_expr)?;
                let v_ptr = ctx
                    .block()
                    .call(I64, "js_url_coerce_string", &[(DOUBLE, &v_v)]);
                ctx.block().call_void(
                    "js_url_search_params_delete2",
                    &[(I64, &p_ptr), (I64, &n_ptr), (I64, &v_ptr)],
                );
            } else {
                ctx.block().call_void(
                    "js_url_search_params_delete",
                    &[(I64, &p_ptr), (I64, &n_ptr)],
                );
            }
            Ok(ctx
                .block()
                .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64))
        }

        Expr::UrlSearchParamsToString(params) => {
            let p_v = lower_expr(ctx, params)?;
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            let str_ptr = ctx
                .block()
                .call(I64, "js_url_search_params_to_string", &[(I64, &p_ptr)]);
            Ok(nanbox_string_inline(ctx.block(), &str_ptr))
        }

        Expr::UrlSearchParamsEntries(params) => {
            // Runtime returns a fully NaN-boxed POINTER_TAG f64, so we pass
            // it through unchanged. See `js_url_search_params_entries_arr`
            // rustdoc.
            let p_v = lower_expr(ctx, params)?;
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            let arr =
                ctx.block()
                    .call(DOUBLE, "js_url_search_params_entries_arr", &[(I64, &p_ptr)]);
            Ok(arr)
        }

        Expr::UrlSearchParamsKeys(params) => {
            let p_v = lower_expr(ctx, params)?;
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            Ok(ctx
                .block()
                .call(DOUBLE, "js_url_search_params_keys_arr", &[(I64, &p_ptr)]))
        }

        Expr::UrlSearchParamsValues(params) => {
            let p_v = lower_expr(ctx, params)?;
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            Ok(ctx
                .block()
                .call(DOUBLE, "js_url_search_params_values_arr", &[(I64, &p_ptr)]))
        }

        Expr::UrlSearchParamsSort(params) => {
            let p_v = lower_expr(ctx, params)?;
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            ctx.block()
                .call_void("js_url_search_params_sort", &[(I64, &p_ptr)]);
            Ok(ctx
                .block()
                .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64))
        }

        Expr::UrlSearchParamsForEach {
            params,
            callback,
            this_arg,
        } => {
            let p_v = lower_expr(ctx, params)?;
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            let cb_v = lower_expr(ctx, callback)?;
            let this_v = if let Some(this_arg) = this_arg {
                lower_expr(ctx, this_arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            ctx.block().call_void(
                "js_url_search_params_for_each",
                &[(I64, &p_ptr), (DOUBLE, &cb_v), (DOUBLE, &this_v)],
            );
            Ok(ctx
                .block()
                .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64))
        }

        Expr::UrlSearchParamsGetAll { params, name } => {
            let p_v = lower_expr(ctx, params)?;
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            let n_v = lower_expr(ctx, name)?;
            let n_ptr = ctx
                .block()
                .call(I64, "js_url_coerce_string", &[(DOUBLE, &n_v)]);
            // Returns f64 with the raw array pointer bit-cast in; the runtime
            // does not NaN-box it, so tag it here with POINTER_TAG.
            let raw_f64 = ctx.block().call(
                DOUBLE,
                "js_url_search_params_get_all",
                &[(I64, &p_ptr), (I64, &n_ptr)],
            );
            let bits = ctx.block().bitcast_double_to_i64(&raw_f64);
            Ok(nanbox_pointer_inline(ctx.block(), &bits))
        }

        Expr::FsRmRecursive(path) => {
            let p = lower_expr(ctx, path)?;
            let _ = ctx.block().call(I32, "js_fs_rm_recursive", &[(DOUBLE, &p)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // -------- V8 / perry-jsruntime interop (issue #248) --------
        // These variants are produced by perry-hir's `transform_js_imports`
        // pass when a TS module imports from a `.js` file the resolver
        // classifies as JS-runtime-loaded (see
        // `crates/perry/src/commands/compile/collect_modules.rs:73`).
        // The runtime FFIs live in `perry-jsruntime/src/interop.rs` and are
        // declared above in runtime_decls.rs. JS values come back as
        // NaN-boxed f64 with V8-handle tag 0x7FFB (handled inside
        // perry-jsruntime — Perry codegen treats them as opaque doubles).
        // Module handles are u64 (deno_core::ModuleId), bitcast through
        // f64 in transit so they share the lower_expr return type.
        //
        // `JsCreateCallback` is intentionally not implemented here —
        // see the bail comment near the catch-all below for the reason.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
