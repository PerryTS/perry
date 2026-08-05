//! URL / URLSearchParams + FsRmRecursive.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I1, I32, I64};

use super::{
    lower_expr, lower_url_string_getter, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64,
    FnCtx,
};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::FileURLToPath(url) => {
            let v = lower_expr(ctx, url)?;
            // 1-arg fast path: pass `undefined` for the options arg (#2975).
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            Ok(ctx.block().call(
                DOUBLE,
                "js_url_file_url_to_path",
                &[(DOUBLE, &v), (DOUBLE, &undef)],
            ))
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
                // `js_url_coerce_string` returns a RAW `StringHeader` pointer,
                // not a NaN-boxed value, so nothing else keeps it alive. Two
                // collection points then stand between it and its use:
                // lowering `base` runs arbitrary user code, and the second
                // coercion allocates whenever `base` is not already a string.
                // An evacuating cycle in either window leaves `url_ptr`
                // pointing at a forwarded object and
                // `js_url_new_with_base` parses freed bytes.
                //
                // Root before the first collection point and re-read after the
                // last, per `docs/src/internals/gc-rooting-invariant.md` — the
                // ordering is the whole fix; adding the root after the coercion
                // would root an already-stale pointer.
                let url_slot = super::temp_root::temp_root_push_i64(ctx, &url_ptr);
                let base_v = lower_expr(ctx, base)?;
                // Layer 1 migration (#7459): `call_rooted` emits the collecting
                // call and roots its result in one step, so no unrooted
                // register for `base_ptr` ever exists to be held across a later
                // collection point. The window that made #7453 a bug is not
                // expressible here.
                let base_slot = crate::rooting::call_rooted(
                    ctx,
                    I64,
                    "js_url_coerce_string",
                    &[(DOUBLE, &base_v)],
                );
                let url_ptr = super::temp_root::temp_root_get_i64(ctx, &url_slot);
                let base_ptr = base_slot.read(ctx);
                let obj = ctx.block().call(
                    I64,
                    "js_url_new_with_base",
                    &[(I64, &url_ptr), (I64, &base_ptr)],
                );
                base_slot.release(ctx);
                super::temp_root::temp_root_truncate(ctx, &url_slot);
                obj
            } else {
                ctx.block().call(I64, "js_url_new", &[(I64, &url_ptr)])
            };
            Ok(nanbox_pointer_inline(ctx.block(), &obj))
        }

        Expr::UrlPatternNew { input, base } => {
            // Same window as `UrlNew` above: `input_v` is a NaN-boxed heap
            // value held in a register while `base` lowers, which can run user
            // code and collect. `lower_exprs_rooted` protects each operand
            // whose later siblings may trigger GC and hands back reloaded
            // values, so nothing crosses the window in a bare register.
            let (input_v, base_v, operand_guard) = if let Some(base) = base {
                let (vals, guard) = super::temp_root::lower_exprs_rooted(ctx, &[input, base])?;
                (vals[0].clone(), vals[1].clone(), guard)
            } else {
                let input_v = lower_expr(ctx, input)?;
                (
                    input_v,
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
                    None,
                )
            };
            let obj = ctx.block().call(
                I64,
                "js_url_pattern_new",
                &[(DOUBLE, &input_v), (DOUBLE, &base_v)],
            );
            // Released only after the last use of both operands.
            super::temp_root::temp_root_release(ctx, operand_guard);
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
            // Same window as the URLSearchParams family (#7462/#7463), and it
            // covers all nine setters at once: `url_handle` is a raw heap
            // pointer and lowering `value` runs arbitrary user code that can
            // collect. Root both, unbox from the reloaded receiver.
            let (vals, operand_guard) = super::temp_root::lower_exprs_rooted(ctx, &[url, value])?;
            let (url_v, val_v) = (vals[0].clone(), vals[1].clone());
            let url_handle = unbox_to_i64(ctx.block(), &url_v);
            ctx.block()
                .call_void(runtime_fn, &[(I64, &url_handle), (DOUBLE, &val_v)]);
            super::temp_root::temp_root_release(ctx, operand_guard);
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

        Expr::UrlSearchParamsMissingArgs {
            params,
            args,
            name_and_value,
        } => {
            let _ = lower_expr(ctx, params)?;
            for arg in args {
                let _ = lower_expr(ctx, arg)?;
            }
            let kind = if *name_and_value { "2" } else { "1" };
            Ok(ctx.block().call(
                DOUBLE,
                "js_url_search_params_throw_missing_args",
                &[(I32, kind)],
            ))
        }

        Expr::UrlSearchParamsGet { params, name } => {
            // #7453's shape: `p_ptr` — and the tagged `p_v` it is masked from —
            // is a heap pointer, and lowering `name` runs arbitrary user code
            // that can collect. Root both operands first and unbox from the
            // reloaded receiver, so neither crosses the window in a register.
            // The guard lives to the end of the arm: every use below is a use
            // of one of the two rooted values.
            let (vals, operand_guard) = super::temp_root::lower_exprs_rooted(ctx, &[params, name])?;
            let (p_v, n_v) = (vals[0].clone(), vals[1].clone());
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            let str_ptr = ctx.block().call(
                I64,
                "js_url_search_params_get",
                &[(I64, &p_ptr), (DOUBLE, &n_v)],
            );
            // Released after the consuming call, which itself allocates.
            super::temp_root::temp_root_release(ctx, operand_guard);
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
            // #7453's shape: `p_ptr` — and the tagged `p_v` it is masked from —
            // is a heap pointer, and lowering `name` runs arbitrary user code
            // that can collect. Root both operands first and unbox from the
            // reloaded receiver, so neither crosses the window in a register.
            // The guard lives to the end of the arm: every use below is a use
            // of one of the two rooted values.
            // All operands rooted together, `value` included when present, so
            // nothing crosses its lowering in a register. #7462 rooted only
            // `params`+`name` and left this path with the window it was meant
            // to close.
            let mut operand_exprs: Vec<&Expr> = vec![params, name];
            if let Some(v_expr) = value {
                operand_exprs.push(v_expr);
            }
            let (vals, operand_guard) = super::temp_root::lower_exprs_rooted(ctx, &operand_exprs)?;
            let (p_v, n_v) = (vals[0].clone(), vals[1].clone());
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            // Runtime returns 0.0 / 1.0 as a plain f64 — not NaN-boxed.
            // Translate to TAG_TRUE / TAG_FALSE so `typeof` and strict-eq
            // behave correctly.
            let raw = if value.is_some() {
                let v_v = vals[2].clone();
                ctx.block().call(
                    DOUBLE,
                    "js_url_search_params_has2",
                    &[(I64, &p_ptr), (DOUBLE, &n_v), (DOUBLE, &v_v)],
                )
            } else {
                ctx.block().call(
                    DOUBLE,
                    "js_url_search_params_has",
                    &[(I64, &p_ptr), (DOUBLE, &n_v)],
                )
            };
            // Released after the consuming call, which itself allocates.
            super::temp_root::temp_root_release(ctx, operand_guard);
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
            // #7453's shape: `p_ptr` — and the tagged `p_v` it is masked from —
            // is a heap pointer, and lowering `name` runs arbitrary user code
            // that can collect. Root both operands first and unbox from the
            // reloaded receiver, so neither crosses the window in a register.
            // The guard lives to the end of the arm: every use below is a use
            // of one of the two rooted values.
            // All operands are rooted together: `value` is lowered too, so
            // nothing crosses it in a register. #7462 rooted only
            // `params`+`name`, which left the three-operand path with the same
            // window it was meant to close.
            let (vals, operand_guard) =
                super::temp_root::lower_exprs_rooted(ctx, &[params, name, value])?;
            let (p_v, n_v, val_v) = (vals[0].clone(), vals[1].clone(), vals[2].clone());
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            ctx.block().call_void(
                "js_url_search_params_set",
                &[(I64, &p_ptr), (DOUBLE, &n_v), (DOUBLE, &val_v)],
            );
            // Released after the consuming call, which itself allocates.
            super::temp_root::temp_root_release(ctx, operand_guard);
            Ok(ctx
                .block()
                .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64))
        }

        Expr::UrlSearchParamsAppend {
            params,
            name,
            value,
        } => {
            // #7453's shape: `p_ptr` — and the tagged `p_v` it is masked from —
            // is a heap pointer, and lowering `name` runs arbitrary user code
            // that can collect. Root both operands first and unbox from the
            // reloaded receiver, so neither crosses the window in a register.
            // The guard lives to the end of the arm: every use below is a use
            // of one of the two rooted values.
            // All operands are rooted together: `value` is lowered too, so
            // nothing crosses it in a register. #7462 rooted only
            // `params`+`name`, which left the three-operand path with the same
            // window it was meant to close.
            let (vals, operand_guard) =
                super::temp_root::lower_exprs_rooted(ctx, &[params, name, value])?;
            let (p_v, n_v, val_v) = (vals[0].clone(), vals[1].clone(), vals[2].clone());
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            ctx.block().call_void(
                "js_url_search_params_append",
                &[(I64, &p_ptr), (DOUBLE, &n_v), (DOUBLE, &val_v)],
            );
            // Released after the consuming call, which itself allocates.
            super::temp_root::temp_root_release(ctx, operand_guard);
            Ok(ctx
                .block()
                .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64))
        }

        Expr::UrlSearchParamsDelete {
            params,
            name,
            value,
        } => {
            // #7453's shape: `p_ptr` — and the tagged `p_v` it is masked from —
            // is a heap pointer, and lowering `name` runs arbitrary user code
            // that can collect. Root both operands first and unbox from the
            // reloaded receiver, so neither crosses the window in a register.
            // The guard lives to the end of the arm: every use below is a use
            // of one of the two rooted values.
            // All operands rooted together, `value` included when present, so
            // nothing crosses its lowering in a register. #7462 rooted only
            // `params`+`name` and left this path with the window it was meant
            // to close.
            let mut operand_exprs: Vec<&Expr> = vec![params, name];
            if let Some(v_expr) = value {
                operand_exprs.push(v_expr);
            }
            let (vals, operand_guard) = super::temp_root::lower_exprs_rooted(ctx, &operand_exprs)?;
            let (p_v, n_v) = (vals[0].clone(), vals[1].clone());
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            if value.is_some() {
                let v_v = vals[2].clone();
                ctx.block().call_void(
                    "js_url_search_params_delete2",
                    &[(I64, &p_ptr), (DOUBLE, &n_v), (DOUBLE, &v_v)],
                );
            } else {
                ctx.block().call_void(
                    "js_url_search_params_delete",
                    &[(I64, &p_ptr), (DOUBLE, &n_v)],
                );
            }
            // Released after the consuming call on BOTH arms. #7462's automated
            // placement put this inside the `else` only, so the with-value path
            // pushed two temp roots per execution and never truncated them —
            // unbounded growth in a loop, and it compiled without a warning.
            super::temp_root::temp_root_release(ctx, operand_guard);
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
            // Two windows here, not one: `p_ptr` crosses the `callback`
            // lowering, and both it and `cb_v` cross `this_arg`'s. Root every
            // operand together and unbox from the reloaded receiver.
            let mut operand_exprs: Vec<&Expr> = vec![params, callback];
            if let Some(this_arg) = this_arg {
                operand_exprs.push(this_arg);
            }
            let (vals, operand_guard) = super::temp_root::lower_exprs_rooted(ctx, &operand_exprs)?;
            let (p_v, cb_v) = (vals[0].clone(), vals[1].clone());
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            let this_v = if this_arg.is_some() {
                vals[2].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            ctx.block().call_void(
                "js_url_search_params_for_each",
                &[(I64, &p_ptr), (DOUBLE, &cb_v), (DOUBLE, &this_v)],
            );
            super::temp_root::temp_root_release(ctx, operand_guard);
            Ok(ctx
                .block()
                .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64))
        }

        Expr::UrlSearchParamsGetAll { params, name } => {
            // #7453's shape: `p_ptr` — and the tagged `p_v` it is masked from —
            // is a heap pointer, and lowering `name` runs arbitrary user code
            // that can collect. Root both operands first and unbox from the
            // reloaded receiver, so neither crosses the window in a register.
            // The guard lives to the end of the arm: every use below is a use
            // of one of the two rooted values.
            let (vals, operand_guard) = super::temp_root::lower_exprs_rooted(ctx, &[params, name])?;
            let (p_v, n_v) = (vals[0].clone(), vals[1].clone());
            let p_ptr = unbox_to_i64(ctx.block(), &p_v);
            // Returns f64 with the raw array pointer bit-cast in; the runtime
            // does not NaN-box it, so tag it here with POINTER_TAG.
            let raw_f64 = ctx.block().call(
                DOUBLE,
                "js_url_search_params_get_all",
                &[(I64, &p_ptr), (DOUBLE, &n_v)],
            );
            // Released after the consuming call, which itself allocates.
            super::temp_root::temp_root_release(ctx, operand_guard);
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
