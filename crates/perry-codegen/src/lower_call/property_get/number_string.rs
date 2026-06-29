//! Number / Buffer / universal `.toString()` PropertyGet dispatch arms.
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
use crate::type_analysis::{is_bigint_expr, is_numeric_expr};
use crate::types::{DOUBLE, I32, I64};

/// Number `.toFixed` / `.toPrecision` / `.toExponential`, Buffer/Number
/// `.toString(encoding|radix)`, and numeric `.toString()` arms. Returns
/// `Ok(Some(_))` when one of these claims the call, otherwise `Ok(None)` so the
/// caller continues down the dispatch tower.
pub(crate) fn try_lower_number_string_methods(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    // Number.prototype.toFixed(decimals) — call js_number_to_fixed.
    // Receiver is any number-typed value; we don't gate on
    // is_numeric_expr because tests often call it on Any locals.
    if property == "toFixed"
        && !is_string_expr(ctx, object)
        && !is_array_expr(ctx, object)
        && !is_native_module_dynamic_index(object)
    {
        let v = lower_expr(ctx, object)?;
        let dec = if let Some(arg) = args.first() {
            lower_expr(ctx, arg)?
        } else {
            double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
        };
        let blk = ctx.block();
        let handle = blk.call(I64, "js_number_to_fixed", &[(DOUBLE, &v), (DOUBLE, &dec)]);
        return Ok(Some(nanbox_string_inline(blk, &handle)));
    }
    // Number.prototype.toPrecision(digits)
    if property == "toPrecision"
        && !is_string_expr(ctx, object)
        && !is_array_expr(ctx, object)
        && !is_native_module_dynamic_index(object)
    {
        let v = lower_expr(ctx, object)?;
        let prec = if let Some(arg) = args.first() {
            lower_expr(ctx, arg)?
        } else {
            double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
        };
        let blk = ctx.block();
        let handle = blk.call(
            I64,
            "js_number_to_precision",
            &[(DOUBLE, &v), (DOUBLE, &prec)],
        );
        return Ok(Some(nanbox_string_inline(blk, &handle)));
    }
    // Number.prototype.toExponential(decimals)
    if property == "toExponential"
        && !is_string_expr(ctx, object)
        && !is_array_expr(ctx, object)
        && !is_native_module_dynamic_index(object)
    {
        let v = lower_expr(ctx, object)?;
        let dec = if let Some(arg) = args.first() {
            lower_expr(ctx, arg)?
        } else {
            double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
        };
        let blk = ctx.block();
        let handle = blk.call(
            I64,
            "js_number_to_exponential",
            &[(DOUBLE, &v), (DOUBLE, &dec)],
        );
        return Ok(Some(nanbox_string_inline(blk, &handle)));
    }
    // Buffer.prototype.toString(encoding) — handled BEFORE the radix
    // path because the encoding arg is a STRING ('utf8'/'hex'/'base64'),
    // not a number. Routing a string arg through `fptosi` produces
    // garbage and the runtime defaults to UTF-8 (the original v0.4.131
    // bug that this test pins). We dispatch via the runtime helper
    // `js_value_to_string_with_encoding` which checks BUFFER_REGISTRY
    // at runtime and falls back to `js_jsvalue_to_string` for
    // non-buffer values.
    if property == "toString"
        && args.len() == 1
        && !is_string_expr(ctx, object)
        && !is_array_expr(ctx, object)
        && !is_date_receiver(ctx, object)
        && is_string_expr(ctx, &args[0])
    {
        let has_user_to_string = receiver_class_name(ctx, object)
            .map(|cls| {
                let mut cur = Some(cls);
                while let Some(c) = cur {
                    if ctx
                        .methods
                        .contains_key(&(c.clone(), "toString".to_string()))
                    {
                        return true;
                    }
                    cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                }
                false
            })
            .unwrap_or(false);
        if !has_user_to_string {
            let v = lower_expr(ctx, object)?;
            // Always lower the raw arg value too: for a Number/BigInt receiver
            // the string is the radix (ToNumber-coerced at runtime, #2864), not
            // an encoding. Disambiguation is by receiver type at runtime.
            let arg_box = lower_expr(ctx, &args[0])?;
            let enc_tag_i32 = if let Expr::String(s) = &args[0] {
                let lower = s.to_ascii_lowercase();
                let tag: i32 = match lower.as_str() {
                    "utf8" | "utf-8" => 0,
                    "hex" => 1,
                    "base64" => 2,
                    "base64url" => 3,
                    "latin1" | "binary" => 4,
                    "ascii" => 5,
                    "utf16le" | "utf-16le" | "ucs2" | "ucs-2" => 6,
                    _ => 0,
                };
                tag.to_string()
            } else {
                let blk = ctx.block();
                blk.call(I32, "js_encoding_tag_from_value", &[(DOUBLE, &arg_box)])
            };
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_value_to_string_with_encoding_or_radix",
                &[(DOUBLE, &v), (I32, &enc_tag_i32), (DOUBLE, &arg_box)],
            );
            return Ok(Some(nanbox_string_inline(blk, &handle)));
        }
    }
    // Number.prototype.toString(radix) — special case where the
    // single arg is the radix (2..36). Routes through
    // js_jsvalue_to_string_radix so `(255).toString(16)` returns
    // "ff" instead of "255".
    if property == "toString"
        && args.len() == 1
        && (is_numeric_expr(ctx, object) || is_bigint_expr(ctx, object))
        && !is_string_expr(ctx, object)
        && !is_array_expr(ctx, object)
        && !is_date_receiver(ctx, object)
    {
        // Only treat as radix call if class doesn't have toString.
        let has_user_to_string = receiver_class_name(ctx, object)
            .map(|cls| {
                let mut cur = Some(cls);
                while let Some(c) = cur {
                    if ctx
                        .methods
                        .contains_key(&(c.clone(), "toString".to_string()))
                    {
                        return true;
                    }
                    cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                }
                false
            })
            .unwrap_or(false);
        if !has_user_to_string {
            let v = lower_expr(ctx, object)?;
            // Pass the *raw* NaN-boxed radix value (not an `fptosi` i32). The
            // runtime performs ECMAScript ToNumber/ToInteger coercion and
            // `RangeError` validation on it (#2864); an `fptosi` here would
            // silently collapse NaN/Infinity/string radices to 0 or garbage.
            let radix_v = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_jsvalue_to_string_radix",
                &[(DOUBLE, &v), (DOUBLE, &radix_v)],
            );
            return Ok(Some(nanbox_string_inline(blk, &handle)));
        }
    }
    // Numeric `.toString()` without a radix. Do not claim arbitrary Any
    // receivers here: plain objects may define an own `toString` method that
    // must be called with the source arguments.
    if property == "toString"
        && args.len() <= 1
        && (is_numeric_expr(ctx, object) || is_bigint_expr(ctx, object))
        && !is_string_expr(ctx, object)
        && !is_array_expr(ctx, object)
        && !is_date_receiver(ctx, object)
    {
        // Check whether the receiver class (if any) defines
        // toString itself or via inheritance.
        let has_user_to_string = receiver_class_name(ctx, object)
            .map(|cls| {
                let mut cur = Some(cls);
                while let Some(c) = cur {
                    if ctx
                        .methods
                        .contains_key(&(c.clone(), "toString".to_string()))
                    {
                        return true;
                    }
                    cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                }
                false
            })
            .unwrap_or(false);
        if !has_user_to_string {
            let v = lower_expr(ctx, object)?;
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            // #3146: an explicit `.toString()` member call must throw a
            // TypeError on a nullish receiver, unlike abstract ToString
            // (`String(x)` / templates). `js_jsvalue_to_string_method`
            // adds only that nullish guard and otherwise matches
            // `js_jsvalue_to_string`.
            let handle = blk.call(I64, "js_jsvalue_to_string_method", &[(DOUBLE, &v)]);
            return Ok(Some(nanbox_string_inline(blk, &handle)));
        }
    }
    Ok(None)
}
