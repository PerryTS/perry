//! Comparison operators.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::{CompareOp, Expr};
use perry_types::Type as HirType;

use crate::type_analysis::{
    expr_may_return_boxed_value_from_raw_f64_fallback, is_bigint_expr, is_bool_expr,
    is_numeric_expr, is_string_expr,
};
use crate::types::{DOUBLE, I32, I64};

use super::{lower_expr, unbox_str_handle, unbox_to_i64, FnCtx};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Compare { op, left, right } => {
            // BigInt comparison fast path: NaN-tagged BIGINT_TAG values
            // are unordered under fcmp (NaN), so `a > b` on two bigints
            // always returns false. Route through js_bigint_cmp which
            // returns -1/0/1 for the three bigint ordering outcomes.
            //
            // For RELATIONAL ops (`<`, `<=`, `>`, `>=`) this direct cmp is only
            // valid when BOTH operands are statically BigInt — `js_bigint_cmp`
            // dereferences both as BigInt pointers. A *mixed* relational like
            // `1n < Infinity` or `0n < "1"` needs the full abstract relational
            // comparison (BigInt-vs-Number / BigInt-vs-String coercion), so it
            // falls through to `js_rel_*` below. Equality (`===`/`==`) keeps the
            // either-side gate (its own cross-type handling is unchanged).
            let is_relational_op = matches!(
                op,
                CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
            );
            // The `js_bigint_cmp` fast path is valid ONLY when BOTH operands are
            // statically BigInt. The previous equality variant fired when *either*
            // side was BigInt and fed `js_bigint_cmp` a non-BigInt operand
            // (`0n != undefined`, `0n == ""`), dereferencing an undefined/string
            // NaN-box as a BigIntHeader → garbage. Mixed-type BigInt equality now
            // falls through to `js_loose_eq` (loose, with full BigInt coercion) /
            // `fcmp` (strict, where a type mismatch is correctly never-equal).
            // Relational mixed-type already fell through to `js_rel_*`.
            let bigint_fast_path = is_bigint_expr(ctx, left) && is_bigint_expr(ctx, right);
            if bigint_fast_path {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_handle = unbox_to_i64(blk, &l);
                let r_handle = unbox_to_i64(blk, &r);
                let cmp = blk.call(I32, "js_bigint_cmp", &[(I64, &l_handle), (I64, &r_handle)]);
                let bit = match op {
                    CompareOp::Lt => blk.icmp_slt(I32, &cmp, "0"),
                    CompareOp::Le => blk.icmp_sle(I32, &cmp, "0"),
                    CompareOp::Gt => blk.icmp_sgt(I32, &cmp, "0"),
                    CompareOp::Ge => blk.icmp_sge(I32, &cmp, "0"),
                    CompareOp::Eq | CompareOp::LooseEq => blk.icmp_eq(I32, &cmp, "0"),
                    CompareOp::Ne | CompareOp::LooseNe => blk.icmp_ne(I32, &cmp, "0"),
                };
                let tagged = blk.select(
                    crate::types::I1,
                    &bit,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged));
            }
            // Boolean equality fast path: NaN-tagged TAG_TRUE/FALSE
            // bits don't compare correctly with fcmp. For
            // ===/!== where EITHER side is statically boolean, compare
            // the raw i64 bits via icmp. icmp on bits also works for
            // any other NaN-tagged value (string ptr, object ptr) when
            // the bool literal is on one side — TAG_TRUE bits never
            // match a string/pointer, so the result is correctly false.
            // STRICT only: for LooseEq/LooseNe, booleans need coercion
            // (false == "" → true) which the later js_loose_eq handles.
            let either_bool = is_bool_expr(ctx, left) || is_bool_expr(ctx, right);
            if either_bool && matches!(op, CompareOp::Eq | CompareOp::Ne) {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_bits = blk.bitcast_double_to_i64(&l);
                let r_bits = blk.bitcast_double_to_i64(&r);
                let bit = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    blk.icmp_ne(I64, &l_bits, &r_bits)
                } else {
                    blk.icmp_eq(I64, &l_bits, &r_bits)
                };
                let tagged = blk.select(
                    crate::types::I1,
                    &bit,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged));
            }
            // Null/Undefined literal fast path: `x === null` / `x === undefined` /
            // `x !== null` etc. Both TAG_NULL and TAG_UNDEFINED are NaN-tagged
            // doubles, so fcmp is unordered (always false) and the string/js_eq
            // fallbacks misclassify these tags as "invalid string → both equal".
            // Compare raw i64 bits directly.
            //
            // For LooseEq/LooseNe (== / !=), null and undefined are loosely
            // equal to each other but not to anything else. Handle that by
            // routing `x == null` to `(bits == TAG_NULL) | (bits == TAG_UNDEF)`.
            let left_is_null = matches!(left.as_ref(), Expr::Null);
            let left_is_undef = matches!(left.as_ref(), Expr::Undefined);
            let right_is_null = matches!(right.as_ref(), Expr::Null);
            let right_is_undef = matches!(right.as_ref(), Expr::Undefined);
            let either_nullish_lit =
                left_is_null || left_is_undef || right_is_null || right_is_undef;
            if either_nullish_lit
                && matches!(
                    op,
                    CompareOp::Eq | CompareOp::Ne | CompareOp::LooseEq | CompareOp::LooseNe
                )
            {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_bits = blk.bitcast_double_to_i64(&l);
                let r_bits = blk.bitcast_double_to_i64(&r);
                let is_loose = matches!(op, CompareOp::LooseEq | CompareOp::LooseNe);
                let bit = if is_loose {
                    // Loose equality: x == null → (x === null) || (x === undefined)
                    let eq_l_r = blk.icmp_eq(I64, &l_bits, &r_bits);
                    let cmp_l_null = blk.icmp_eq(I64, &l_bits, crate::nanbox::TAG_NULL_I64);
                    let cmp_l_undef = blk.icmp_eq(I64, &l_bits, crate::nanbox::TAG_UNDEFINED_I64);
                    let cmp_r_null = blk.icmp_eq(I64, &r_bits, crate::nanbox::TAG_NULL_I64);
                    let cmp_r_undef = blk.icmp_eq(I64, &r_bits, crate::nanbox::TAG_UNDEFINED_I64);
                    let l_nullish = blk.or(crate::types::I1, &cmp_l_null, &cmp_l_undef);
                    let r_nullish = blk.or(crate::types::I1, &cmp_r_null, &cmp_r_undef);
                    let both_nullish = blk.and(crate::types::I1, &l_nullish, &r_nullish);
                    blk.or(crate::types::I1, &eq_l_r, &both_nullish)
                } else {
                    // Strict equality: bit-exact compare
                    blk.icmp_eq(I64, &l_bits, &r_bits)
                };
                let bit_final = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    blk.xor(crate::types::I1, &bit, "true")
                } else {
                    bit
                };
                let tagged = blk.select(
                    crate::types::I1,
                    &bit_final,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged));
            }
            // "One side is statically string, other is unknown"
            // fallback: `c === Color.Red` where Color is a const
            // object. Neither js_eq (bit-compare, wrong for string
            // content) nor fcmp (NaN-tagged, always false) works.
            //
            // Dispatch through js_string_equals after extracting
            // both string pointers via js_get_string_pointer_unified.
            // That helper returns null for non-string NaN-tagged
            // values, which js_string_equals treats as "not equal"
            // — the correct answer when the unknown side isn't a
            // string at runtime.
            let both_strings_check = is_string_expr(ctx, left) && is_string_expr(ctx, right);
            // The non-statically-string operand collides through this
            // fast path when, at runtime, it is ALSO a non-string. Both
            // operands then funnel through `js_get_string_pointer_unified`,
            // which returns 0 for any non-string NaN-boxed value (numbers,
            // class refs / InjectionTokens, plain objects, …). The
            // subsequent `js_string_equals(0, 0)` returns 1 (its
            // pointer-identity / both-null branches both report "equal"),
            // so two *distinct* non-string values wrongly compare `===`.
            //
            // This is exactly the NestJS DI `token === name` bug:
            // `name` is statically `string` (the destructured
            // `dependencyContext.name`) but at runtime holds a class ref
            // (e.g. `AppService`), and `token` is `any` holding a
            // *different* class ref (`AppController`) — both coerce to 0
            // and the inline `===` reports `true`, throwing
            // `UnknownDependencies` and aborting the app.
            //
            // The static `string` type is therefore a lie here (like the
            // #3576 number-vs-object case). When the OTHER operand is
            // statically `Any` (its runtime value is unconstrained and may
            // be a non-string), this fast path is unsound: route through
            // `js_eq`, which content-compares real strings (SSO + heap) AND
            // correctly distinguishes class refs / objects by identity.
            let other_side_is_any = |other: &Expr| -> bool {
                matches!(
                    crate::type_analysis::static_type_of(ctx, other),
                    Some(HirType::Any) | None
                )
            };
            let one_side_string = !both_strings_check
                && ((is_string_expr(ctx, left)
                    && !is_numeric_expr(ctx, right)
                    && !is_bool_expr(ctx, right)
                    && !other_side_is_any(right))
                    || (is_string_expr(ctx, right)
                        && !is_numeric_expr(ctx, left)
                        && !is_bool_expr(ctx, left)
                        && !other_side_is_any(left)));
            // Only STRICT eq/ne use this string-pointer fast path. Loose `==`/`!=`
            // must fall through to `js_loose_eq` below: when one side is a boxed
            // String/primitive *wrapper* (a POINTER_TAG object, not a STRING_TAG
            // value), `js_get_string_pointer_unified` returns the raw ObjectHeader
            // pointer and `js_string_equals` reads it as a bogus string → wrong
            // result (`new String("x") == "x"` was `false`). `js_loose_eq` unboxes
            // the wrapper first. Strict `=== "lit"` is unaffected (both sides are
            // real strings at runtime). #boxed-loose-eq.
            if one_side_string && matches!(op, CompareOp::Eq | CompareOp::Ne) {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_handle = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &l)]);
                let r_handle = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &r)]);
                let i32_eq = blk.call(
                    I32,
                    "js_string_equals",
                    &[(I64, &l_handle), (I64, &r_handle)],
                );
                let bit = blk.icmp_ne(I32, &i32_eq, "0");
                let bit_final = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    blk.xor(crate::types::I1, &bit, "true")
                } else {
                    bit
                };
                let tagged = blk.select(
                    crate::types::I1,
                    &bit_final,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged));
            }
            // Generic equality fallback: when neither operand is
            // statically numeric, dispatch through js_eq which
            // handles strings, booleans, objects, null, undefined
            // via NaN-tag inspection. Used by `eq` helpers in tests
            // that take `any` and pass NaN-tagged values.
            let either_non_numeric = !is_numeric_expr(ctx, left) && !is_numeric_expr(ctx, right);
            let only_eq = matches!(
                op,
                CompareOp::Eq | CompareOp::LooseEq | CompareOp::Ne | CompareOp::LooseNe
            );
            // We still let the more specific paths below win for
            // statically-typed string/bool operands; this fallback
            // only handles the truly-Any case.
            let unknown_l = !is_numeric_expr(ctx, left)
                && !is_string_expr(ctx, left)
                && !is_bool_expr(ctx, left);
            let unknown_r = !is_numeric_expr(ctx, right)
                && !is_string_expr(ctx, right)
                && !is_bool_expr(ctx, right);
            if either_non_numeric && only_eq && unknown_l && unknown_r {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                // Use js_loose_eq for == / != (handles null==undefined,
                // cross-type coercion). Use js_eq for === / !==.
                let eq_fn = if matches!(op, CompareOp::LooseEq | CompareOp::LooseNe) {
                    "js_loose_eq"
                } else {
                    "js_eq"
                };
                let l_bits = blk.bitcast_double_to_i64(&l);
                let r_bits = blk.bitcast_double_to_i64(&r);
                let result_bits = blk.call(I64, eq_fn, &[(I64, &l_bits), (I64, &r_bits)]);
                let result = blk.bitcast_i64_to_double(&result_bits);
                if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    let cmp = blk.icmp_eq(I64, &result_bits, crate::nanbox::TAG_TRUE_I64);
                    let inv = blk.xor(crate::types::I1, &cmp, "true");
                    let tagged = blk.select(
                        crate::types::I1,
                        &inv,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged));
                }
                return Ok(result);
            }

            // String equality fast path: fcmp doesn't work on
            // NaN-tagged string pointers (NaN comparisons are
            // unordered → always false). When both operands are
            // statically strings, dispatch through js_string_equals.
            let both_strings = is_string_expr(ctx, left) && is_string_expr(ctx, right);
            if both_strings
                && matches!(
                    op,
                    CompareOp::Eq | CompareOp::LooseEq | CompareOp::Ne | CompareOp::LooseNe
                )
            {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                // Issue #214: SSO-safe unbox — the inline mask returns
                // garbage for SHORT_STRING_TAG values (e.g. SSO results
                // from `JSON.parse('["hello"]')[0]`), causing
                // `js_string_equals` to deref the inline payload bytes.
                let l_handle = unbox_str_handle(blk, &l);
                let r_handle = unbox_str_handle(blk, &r);
                let i32_eq = blk.call(
                    I32,
                    "js_string_equals",
                    &[(I64, &l_handle), (I64, &r_handle)],
                );
                let bit = blk.icmp_ne(I32, &i32_eq, "0");
                let bit_final = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    blk.xor(crate::types::I1, &bit, "true")
                } else {
                    bit
                };
                let tagged_i64 = blk.select(
                    crate::types::I1,
                    &bit_final,
                    crate::types::I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged_i64));
            }
            // String relational fast path: `s1 < s2`, `s1 > s2`, etc.
            // fcmp on NaN-tagged pointers is unordered (always false),
            // so dispatch through js_string_compare which returns
            // -1/0/1 like memcmp. Then test the result against 0 with
            // the right icmp predicate.
            if both_strings
                && matches!(
                    op,
                    CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
                )
            {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                // Issue #214: SSO-safe unbox.
                let l_handle = unbox_str_handle(blk, &l);
                let r_handle = unbox_str_handle(blk, &r);
                let cmp_i32 = blk.call(
                    I32,
                    "js_string_compare",
                    &[(I64, &l_handle), (I64, &r_handle)],
                );
                let bit = match op {
                    CompareOp::Lt => blk.icmp_slt(I32, &cmp_i32, "0"),
                    CompareOp::Le => blk.icmp_sle(I32, &cmp_i32, "0"),
                    CompareOp::Gt => blk.icmp_sgt(I32, &cmp_i32, "0"),
                    CompareOp::Ge => blk.icmp_sge(I32, &cmp_i32, "0"),
                    _ => unreachable!(),
                };
                let tagged_i64 = blk.select(
                    crate::types::I1,
                    &bit,
                    crate::types::I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged_i64));
            }

            // Loose equality (==, !=): dispatch through js_loose_eq
            // which handles cross-type coercion (null==undefined,
            // "1"==1, false==0, etc.). Strict === already handled
            // above by the typed fast paths.
            if matches!(op, CompareOp::LooseEq | CompareOp::LooseNe) {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_bits = blk.bitcast_double_to_i64(&l);
                let r_bits = blk.bitcast_double_to_i64(&r);
                let result_bits = blk.call(I64, "js_loose_eq", &[(I64, &l_bits), (I64, &r_bits)]);
                if matches!(op, CompareOp::LooseNe) {
                    let cmp = blk.icmp_eq(I64, &result_bits, crate::nanbox::TAG_TRUE_I64);
                    let inv = blk.xor(crate::types::I1, &cmp, "true");
                    let tagged = blk.select(
                        crate::types::I1,
                        &inv,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged));
                }
                return Ok(blk.bitcast_i64_to_double(&result_bits));
            }

            // An ordered relational compare (`<`, `<=`, `>`, `>=`) whose
            // operands aren't BOTH statically numeric needs the full ECMAScript
            // Abstract Relational Comparison: ToPrimitive (`{valueOf}`/`Date`),
            // lexicographic string compare, BigInt-vs-Number/String coercion,
            // and null/boolean/string ToNumber. A bare `fcmp` mishandles all of
            // these (NaN-boxed operands are unordered → always `false`). Route
            // through the runtime `js_rel_*` helpers, which return a NaN-boxed
            // boolean. The statically-numeric case keeps the bare `fcmp` fast
            // path below (and Dates are subsumed — they aren't numeric_expr).
            let both_numeric = is_numeric_expr(ctx, left)
                && is_numeric_expr(ctx, right)
                && !expr_may_return_boxed_value_from_raw_f64_fallback(ctx, left)
                && !expr_may_return_boxed_value_from_raw_f64_fallback(ctx, right)
                && !is_bigint_expr(ctx, left)
                && !is_bigint_expr(ctx, right);
            if is_relational_op && !both_numeric {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let fname = match op {
                    CompareOp::Lt => "js_rel_lt",
                    CompareOp::Le => "js_rel_le",
                    CompareOp::Gt => "js_rel_gt",
                    CompareOp::Ge => "js_rel_ge",
                    _ => unreachable!(),
                };
                let res = blk.call(DOUBLE, fname, &[(DOUBLE, &l), (DOUBLE, &r)]);
                return Ok(res);
            }
            // Strict ===/!== where the operands are NOT both certainly
            // numeric must NOT fall to the bare fcmp tail: a declared
            // `Number` local can carry an object at runtime (`var a = 2;
            // f(){ a = o; } f(); a === o` — the static type lies, and fcmp
            // on NaN-boxed pointers is unordered → permanently false).
            // js_eq answers correctly for every runtime shape, including
            // the honest number-vs-object case (#3576 probe family).
            if matches!(op, CompareOp::Eq | CompareOp::Ne) && !both_numeric {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_bits = blk.bitcast_double_to_i64(&l);
                let r_bits = blk.bitcast_double_to_i64(&r);
                let result_bits = blk.call(I64, "js_eq", &[(I64, &l_bits), (I64, &r_bits)]);
                if matches!(op, CompareOp::Ne) {
                    let cmp = blk.icmp_eq(I64, &result_bits, crate::nanbox::TAG_TRUE_I64);
                    let inv = blk.xor(crate::types::I1, &cmp, "true");
                    let tagged = blk.select(
                        crate::types::I1,
                        &inv,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged));
                }
                return Ok(blk.bitcast_i64_to_double(&result_bits));
            }
            let l = lower_expr(ctx, left)?;
            let r = lower_expr(ctx, right)?;
            let pred = match op {
                CompareOp::Eq => "oeq",
                // !== uses `une` (unordered or not equal), NOT `one`.
                // `one` is "ordered and not equal" which returns false
                // when either operand is NaN. JS !== on NaN must return
                // true: NaN !== NaN → !(NaN === NaN) → !false → true.
                CompareOp::Ne => "une",
                CompareOp::Lt => "olt",
                CompareOp::Le => "ole",
                CompareOp::Gt => "ogt",
                CompareOp::Ge => "oge",
                // LooseEq/Ne handled above
                CompareOp::LooseEq | CompareOp::LooseNe => unreachable!(),
            };
            let blk = ctx.block();
            let bit = blk.fcmp(pred, &l, &r);
            let tag_true_i64 = crate::nanbox::TAG_TRUE_I64;
            let tag_false_i64 = crate::nanbox::TAG_FALSE_I64;
            let tagged_i64 = blk.select(
                crate::types::I1,
                &bit,
                crate::types::I64,
                tag_true_i64,
                tag_false_i64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged_i64))
        }

        // -------- Objects (Phase B.4) --------
        // `{ k1: v1, k2: v2, … }` literal: allocate, set each field by
        // name (key string sourced from the StringPool), NaN-box the
        // pointer via js_nanbox_pointer.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
