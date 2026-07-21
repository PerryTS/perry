//! Map / Set method dispatch + Map/Set/URLSearchParams `.forEach`.
//! Pure code move from `property_get.rs` — no behavior change.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::type_analysis::{is_map_expr, is_set_expr, is_url_search_params_expr};
use crate::types::{DOUBLE, I64};

/// Map/Set methods on PropertyGet receivers. The HIR only folds
/// `m.set(...)`/`m.get(...)` to MapSet/MapGet when `m` is an Ident receiver.
/// When the receiver is `this.field` (class method accessing a Map-typed
/// field), the generic Call reaches here and needs an explicit dispatch.
pub(crate) fn try_lower_map_set_methods(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    if is_map_expr(ctx, object) {
        match property {
            "set" if args.len() == 2 => {
                let m_box = lower_expr(ctx, object)?;
                let k_box = lower_expr(ctx, &args[0])?;
                let v_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                blk.call_void(
                    "js_map_set",
                    &[(I64, &m_handle), (DOUBLE, &k_box), (DOUBLE, &v_box)],
                );
                return Ok(Some(m_box));
            }
            "get" if args.len() == 1 => {
                let m_box = lower_expr(ctx, object)?;
                let k_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                return Ok(Some(blk.call(
                    DOUBLE,
                    "js_map_get",
                    &[(I64, &m_handle), (DOUBLE, &k_box)],
                )));
            }
            "has" if args.len() == 1 => {
                let m_box = lower_expr(ctx, object)?;
                let k_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                let i32_v = blk.call(
                    crate::types::I32,
                    "js_map_has",
                    &[(I64, &m_handle), (DOUBLE, &k_box)],
                );
                return Ok(Some(crate::expr::i32_bool_to_nanbox(blk, &i32_v)));
            }
            "delete" if args.len() == 1 => {
                let m_box = lower_expr(ctx, object)?;
                let k_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                let i32_v = blk.call(
                    crate::types::I32,
                    "js_map_delete",
                    &[(I64, &m_handle), (DOUBLE, &k_box)],
                );
                return Ok(Some(crate::expr::i32_bool_to_nanbox(blk, &i32_v)));
            }
            "clear" if args.is_empty() => {
                let m_box = lower_expr(ctx, object)?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                blk.call_void("js_map_clear", &[(I64, &m_handle)]);
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            // Map iterator methods (entries / keys / values).
            // Issue #412: the HIR-level fold at expr_call.rs only
            // fires for `Expr::Ident` receivers (a plain local).
            // Receivers like `new Map(...).values()`,
            // `this.field.values()`, `obj.field.values()` come
            // through the generic call path and need codegen-time
            // dispatch — pre-fix they fell off the bottom of the
            // method-dispatch tower and silently returned
            // `undefined`. The runtime returns a real Array; we
            // NaN-box-pointer the result for downstream
            // `.length` / `forEach` / `Array.from` use.
            // #2856: a value-level `.entries()`/`.keys()`/`.values()` call
            // returns a real iterator OBJECT (`.next()`-bearing, not an
            // Array). The eager Array materializers (`js_map_entries` etc.)
            // are still used by the for-of/spread fast paths via the
            // `Expr::MapEntries`/etc HIR variants.
            "entries" | "keys" | "values" if args.is_empty() => {
                let m_box = lower_expr(ctx, object)?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                let runtime_fn = match property {
                    "entries" => "js_map_entries_iter_obj",
                    "keys" => "js_map_keys_iter_obj",
                    "values" => "js_map_values_iter_obj",
                    _ => unreachable!(),
                };
                let result = blk.call(I64, runtime_fn, &[(I64, &m_handle)]);
                return Ok(Some(crate::expr::nanbox_pointer_inline_pub(blk, &result)));
            }
            _ => {}
        }
    }
    if is_set_expr(ctx, object) {
        match property {
            "add" if args.len() == 1 => {
                let s_box = lower_expr(ctx, object)?;
                let v_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                blk.call_void("js_set_add", &[(I64, &s_handle), (DOUBLE, &v_box)]);
                return Ok(Some(s_box));
            }
            "has" if args.len() == 1 => {
                let s_box = lower_expr(ctx, object)?;
                let v_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                let i32_v = blk.call(
                    crate::types::I32,
                    "js_set_has",
                    &[(I64, &s_handle), (DOUBLE, &v_box)],
                );
                return Ok(Some(crate::expr::i32_bool_to_nanbox(blk, &i32_v)));
            }
            "delete" if args.len() == 1 => {
                let s_box = lower_expr(ctx, object)?;
                let v_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                let i32_v = blk.call(
                    crate::types::I32,
                    "js_set_delete",
                    &[(I64, &s_handle), (DOUBLE, &v_box)],
                );
                return Ok(Some(crate::expr::i32_bool_to_nanbox(blk, &i32_v)));
            }
            "clear" if args.is_empty() => {
                let s_box = lower_expr(ctx, object)?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                blk.call_void("js_set_clear", &[(I64, &s_handle)]);
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            // Set iterator methods. Per ECMA-262 §24.2.3.5–7,
            // `Set.prototype.values`, `.keys`, and `.entries` all
            // return iterators over the Set's elements (keys ===
            // values for Sets; entries yields [v, v] pairs).
            // Perry's `js_set_to_array` returns a real Array of
            // the Set's elements — sufficient for the common
            // `Array.from(s.values())` / `for-of s.values()` /
            // spread shapes. Pre-fix `new Set([1]).values()`
            // returned `undefined` because the HIR-level fold at
            // expr_call.rs only fires for `Expr::Ident` receivers.
            // #2856: value-level Set iterator methods return real iterator
            // objects. `entries` was previously missing here and on the
            // typed-Set HIR path; for Sets `entries` yields `[v, v]` pairs.
            "values" | "keys" | "entries" if args.is_empty() => {
                let s_box = lower_expr(ctx, object)?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                let runtime_fn = match property {
                    "values" => "js_set_values_iter_obj",
                    "keys" => "js_set_keys_iter_obj",
                    "entries" => "js_set_entries_iter_obj",
                    _ => unreachable!(),
                };
                let result = blk.call(I64, runtime_fn, &[(I64, &s_handle)]);
                return Ok(Some(crate::expr::nanbox_pointer_inline_pub(blk, &result)));
            }
            // #2872: ES2024 Set composition methods. union/intersection/
            // difference/symmetricDifference take a set-like `other` and
            // return a NEW Set; isSubsetOf/isSupersetOf/isDisjointFrom return
            // a boolean. The runtime fns receive the receiver as an I64 set
            // handle and `other` as a NaN-boxed f64.
            "union" | "intersection" | "difference" | "symmetricDifference" if args.len() == 1 => {
                let s_box = lower_expr(ctx, object)?;
                let other_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                let runtime_fn = match property {
                    "union" => "js_set_union",
                    "intersection" => "js_set_intersection",
                    "difference" => "js_set_difference",
                    "symmetricDifference" => "js_set_symmetric_difference",
                    _ => unreachable!(),
                };
                let result = blk.call(I64, runtime_fn, &[(I64, &s_handle), (DOUBLE, &other_box)]);
                return Ok(Some(crate::expr::nanbox_pointer_inline_pub(blk, &result)));
            }
            "isSubsetOf" | "isSupersetOf" | "isDisjointFrom" if args.len() == 1 => {
                let s_box = lower_expr(ctx, object)?;
                let other_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                let runtime_fn = match property {
                    "isSubsetOf" => "js_set_is_subset_of",
                    "isSupersetOf" => "js_set_is_superset_of",
                    "isDisjointFrom" => "js_set_is_disjoint_from",
                    _ => unreachable!(),
                };
                let i32_v = blk.call(
                    crate::types::I32,
                    runtime_fn,
                    &[(I64, &s_handle), (DOUBLE, &other_box)],
                );
                return Ok(Some(crate::expr::i32_bool_to_nanbox(blk, &i32_v)));
            }
            _ => {}
        }
    }
    Ok(None)
}

/// Map.forEach / Set.forEach / URLSearchParams.forEach. The HIR emits these as
/// generic `Call { callee: PropertyGet }` because it skips ArrayForEach when
/// the receiver is Map/Set/URLSearchParams. Route to the runtime forEach
/// implementations.
pub(crate) fn try_lower_collection_foreach(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    if property == "forEach" && !args.is_empty() {
        // #2830: lower the optional `thisArg` (args[1]) and pass it through
        // so the callback's `this` is bound; the runtime calls the callback
        // with the full `(value, key, collection)` triple. Map.forEach
        // returns `undefined`.
        if is_map_expr(ctx, object) {
            let m_box = lower_expr(ctx, object)?;
            let cb_box = lower_expr(ctx, &args[0])?;
            let this_arg = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            blk.call_void(
                "js_map_foreach",
                &[(I64, &m_handle), (DOUBLE, &cb_box), (DOUBLE, &this_arg)],
            );
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
        if is_set_expr(ctx, object) {
            let s_box = lower_expr(ctx, object)?;
            let cb_box = lower_expr(ctx, &args[0])?;
            let this_arg = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            blk.call_void(
                "js_set_foreach",
                &[(I64, &s_handle), (DOUBLE, &cb_box), (DOUBLE, &this_arg)],
            );
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
        // URLSearchParams.forEach((value, key, this) => …). The HIR
        // variant `Expr::UrlSearchParamsForEach` only fires when the
        // receiver is a typed-named local; chained access (`u.searchParams
        // .forEach(...)`) and unannotated `const sp = new URLSearchParams()`
        // routes flow through this generic Call path. Route both via the
        // runtime entry so the callback gets the string `(value, key)`
        // pair instead of `(NaN, 0)` from the Array.forEach fast path.
        if is_url_search_params_expr(ctx, object) {
            let p_box = lower_expr(ctx, object)?;
            let cb_box = lower_expr(ctx, &args[0])?;
            let this_arg = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            blk.call_void(
                "js_url_search_params_for_each",
                &[(I64, &p_handle), (DOUBLE, &cb_box), (DOUBLE, &this_arg)],
            );
            return Ok(Some(double_literal(0.0)));
        }
    }
    Ok(None)
}
