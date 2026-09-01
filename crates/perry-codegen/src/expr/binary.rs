//! Binary arithmetic / bitwise / string-concat dispatch.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, CompareOp, Expr, LogicalOp};

use crate::lower_string_concat::{
    flatten_string_add_chain, lower_string_coerce_concat, lower_string_concat,
    lower_string_concat_chain,
};
use crate::native_value::{
    materialize_small_bigint_pointer_to_js_value, BufferAccessMode, LoweredValue,
    MaterializationReason,
};
use crate::type_analysis::{
    add_operands_have_pod_materialization_hazard,
    expr_may_return_boxed_value_from_raw_f64_fallback, is_bigint_expr, is_bool_expr,
    is_numeric_expr, numeric_proof_is_declared_only,
};
use crate::types::{DOUBLE, I1, I128, I32, I64};

use crate::rooting::with_operands_rooted;

use super::{is_known_i32_range, lower_expr, FnCtx};

/// `helper(left, right)` with each operand rooted across the other's lowering
/// and the group released on every path out (#6951).
///
/// All five dynamic-dispatch arms below are this one shape — the operand pair
/// feeds a runtime helper that runs `ToPrimitive` / `ToNumeric` on both sides,
/// so a pointer-bearing left operand has to survive the right operand's
/// evaluation. Before #7615 slice 8 each spelled it out as
/// `lower_operand_pair_rooted` + a `temp_root_release` on its own `return`
/// path; that is five chances to place the release wrong, and #7462 is what
/// one misplaced release costs.
fn lower_rooted_dynamic_binary(
    ctx: &mut FnCtx<'_>,
    helper: &str,
    left: &Expr,
    right: &Expr,
) -> Result<String> {
    with_operands_rooted(ctx, &[left, right], |ctx, values| {
        Ok(ctx.block().call(
            DOUBLE,
            helper,
            &[(DOUBLE, &values[0]), (DOUBLE, &values[1])],
        ))
    })
}

/// Emit JS numeric remainder with an inline fast path for i32-valued operands.
///
/// LLVM lowers `frem double` to a libm `fmod` call on AArch64. Most application
/// remainders use small whole numbers (loop indices, lengths, parsed integer
/// configuration), so prove that common case at runtime and use integer
/// remainder instead. The ordered range checks are deliberately in a separate
/// predecessor from `fptosi`: converting NaN, infinity, or an out-of-range
/// double is poison in LLVM even when a later select would discard it.
fn lower_checked_i32_modulo(ctx: &mut FnCtx<'_>, left: &str, right: &str) -> String {
    let convert_idx = ctx.new_block("mod.i32.convert");
    let integer_idx = ctx.new_block("mod.i32.integer");
    let fallback_idx = ctx.new_block("mod.f64.fallback");
    let merge_idx = ctx.new_block("mod.merge");
    let convert_label = ctx.block_label(convert_idx);
    let integer_label = ctx.block_label(integer_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);

    let min = crate::nanbox::double_literal(f64::from(i32::MIN));
    let max = crate::nanbox::double_literal(f64::from(i32::MAX));
    let l_ge_min = ctx.block().fcmp("oge", left, &min);
    let l_le_max = ctx.block().fcmp("ole", left, &max);
    let r_ge_min = ctx.block().fcmp("oge", right, &min);
    let r_le_max = ctx.block().fcmp("ole", right, &max);
    let l_in_range = ctx.block().and(I1, &l_ge_min, &l_le_max);
    let r_in_range = ctx.block().and(I1, &r_ge_min, &r_le_max);
    let both_in_range = ctx.block().and(I1, &l_in_range, &r_in_range);
    ctx.block()
        .cond_br(&both_in_range, &convert_label, &fallback_label);

    ctx.current_block = convert_idx;
    let li = ctx.block().fptosi(DOUBLE, left, I32);
    let ri = ctx.block().fptosi(DOUBLE, right, I32);
    let l_roundtrip = ctx.block().sitofp(I32, &li, DOUBLE);
    let r_roundtrip = ctx.block().sitofp(I32, &ri, DOUBLE);
    let l_is_integer = ctx.block().fcmp("oeq", &l_roundtrip, left);
    let r_is_integer = ctx.block().fcmp("oeq", &r_roundtrip, right);
    let r_is_nonzero = ctx.block().icmp_ne(I32, &ri, "0");
    let both_integer = ctx.block().and(I1, &l_is_integer, &r_is_integer);
    let can_use_integer = ctx.block().and(I1, &both_integer, &r_is_nonzero);
    ctx.block()
        .cond_br(&can_use_integer, &integer_label, &fallback_label);

    ctx.current_block = integer_idx;
    let remainder = ctx.block().srem(I32, &li, &ri);
    let result = ctx.block().sitofp(I32, &remainder, DOUBLE);

    // Integer remainder loses the sign of a zero result. Test the dividend's
    // IEEE-754 sign bit rather than `left < 0`, because -0 must also produce
    // -0. Nonzero results already have the dividend's sign via signed `srem`.
    let remainder_is_zero = ctx.block().icmp_eq(I32, &remainder, "0");
    let left_bits = ctx.block().bitcast_double_to_i64(left);
    let dividend_has_sign = ctx.block().icmp_slt(I64, &left_bits, "0");
    let needs_negative_zero = ctx.block().and(I1, &remainder_is_zero, &dividend_has_sign);
    let negative_result = ctx.block().fneg(&result);
    let integer_value =
        ctx.block()
            .select(I1, &needs_negative_zero, DOUBLE, &negative_result, &result);
    let integer_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = fallback_idx;
    let fallback_value = ctx.block().frem(left, right);
    let fallback_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (&integer_value, &integer_end),
            (&fallback_value, &fallback_end),
        ],
    )
}

/// A `+` tree whose numeric interpretation needs a runtime confirmation.
///
/// Most callers arrive here because both operands are statically numeric but
/// at least one is numeric only because a DECLARED type said so (#7773,
/// #7776). The other caller is the null-defaulted dynamic value recognized by
/// [`is_null_defaulted_local_plus_numeric_literal`] (#8607): its common value
/// is numeric, but its generic source left the HIR type as `Any`.
///
/// Nothing enforces annotations at runtime, so a `x: number` slot reached
/// through `as any` really can hold a string — and then the spec says `+` is
/// string concatenation, which is what Node does. Trusting the annotation cost
/// two different wrong answers, both silent:
///
/// * `o.x + 1` produced `NaN`, because the number-context read's cold arm
///   `js_number_coerce`s unconditionally; Node prints `s1`.
/// * through a refined local the add did not even coerce — `fadd` on a
///   NaN-BOXED value propagates the input payload on both AArch64 and x86-64,
///   so the string came back out of the add still a string, and the `+ 1`
///   looked like it had evaporated.
///
/// So re-check at runtime instead of assuming. The fast arm keeps the inline
/// `fadd`; only a value that is not a canonical double reaches the dynamic
/// helper, which is the one that implements the spec's `+`.
///
/// **The whole `+` TREE becomes one diamond, not one per node.** That is a
/// correctness-neutral but performance-critical detail, and doing it the
/// obvious way first is what showed why. Per-node diamonds make the outer add
/// of `s += o.x + 1` consume a PHI, and LLVM cannot prove a phi over
/// (`fadd`, runtime call) is a canonical double — so the outer test never
/// folded, its cold arm stayed live in the loop, and `Acc.run`'s hot loop lost
/// its `fadd` to an unconditional call. Measured on the bench mini that shape
/// went 86 ms -> 119 ms. Fusing the tree removes the phi entirely: one test
/// over the tree's violable LEAVES, one branch, then either all-`fadd` or
/// all-`js_dynamic_string_or_number_add`.
///
/// Associativity is preserved rather than assumed away: both arms rebuild the
/// ORIGINAL tree shape. `1 + (2 + "x")` is `"12x"` and `(1 + 2) + "x"` is
/// `"3x"`, so a flattened re-association would be a wrong answer — the leaves
/// are collected in evaluation order for rooting, but the arms are rebuilt
/// node-for-node.
///
/// Every leaf is tested EXCEPT those that `expr_produces_canonical_raw_f64`
/// vouches for (literals, `Math.*`, an explicit coerce, non-`+` arithmetic).
/// Testing only the declared-only leaves is not enough, and the accumulator is
/// the counter-example: `let s = 0; s += r.x + r.y` types `s` as `Number`, but
/// the moment this very lowering's cold arm concatenates, `s` HOLDS A STRING
/// while its static type still says otherwise. Skipping it summed
/// `16zw1113151719` down to `16zw` — the fast arm `fadd`ed a NaN-boxed string
/// and passed it through unchanged, which is the original bug reintroduced one
/// level up. `expr_produces_canonical_raw_f64` declines to vouch for a
/// `LocalGet` precisely because a local is a slot somebody can store into.
///
/// The residual cost lands where it is already small: every read that reaches
/// here is one the compiler could NOT prove, so it pays an inline header
/// precheck or a `js_typed_feedback_class_field_get_guard` call for its shape
/// check regardless. Proven raw-f64 tiers need no guard at all.
fn lower_guarded_numeric_add(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    let mut leaves = Vec::new();
    add_tree_leaves(expr, &mut leaves);
    let needs_test: Vec<bool> = leaves
        .iter()
        .map(|leaf| !crate::type_analysis::expr_produces_canonical_raw_f64(ctx, leaf))
        .collect();

    with_operands_rooted(ctx, &leaves, |ctx, values| {
        let mut cond: Option<String> = None;
        for (value, is_tested) in values.iter().zip(needs_test.iter()) {
            if !is_tested {
                continue;
            }
            let is_num = crate::stmt::emit_js_value_is_number(ctx, value);
            cond = Some(match cond {
                Some(prev) => ctx.block().and(I1, &prev, &is_num),
                None => is_num,
            });
        }
        // Every leaf vouched for: the whole tree is proven canonical raw
        // f64, so the diamond has nothing to test — emit the fast tree
        // unguarded. This is not new trust: the MIXED case already `fadd`s
        // every vouched leaf without a runtime test, so an unsound vouching
        // predicate ships wrong answers there regardless — the predicate's
        // own tests are the guard, not this function. (This corner used to
        // `bail!` as a drift tripwire; #9033's integer-provenance LocalGet
        // arm made it reachable from ordinary code — `width += 1` where
        // `width` is provenance-proven — and the tripwire took whole
        // application builds down: pi's `graphemeWidth`, cc's cli bundle.)
        let Some(all_num) = cond else {
            return Ok(rebuild_add_tree(ctx, expr, values, &mut 0, true));
        };

        let fast_idx = ctx.new_block("guarded_add.numeric");
        let slow_idx = ctx.new_block("guarded_add.dynamic");
        let merge_idx = ctx.new_block("guarded_add.merge");
        let fast_label = ctx.block_label(fast_idx);
        let slow_label = ctx.block_label(slow_idx);
        let merge_label = ctx.block_label(merge_idx);
        ctx.block().cond_br(&all_num, &fast_label, &slow_label);

        ctx.current_block = fast_idx;
        let fast_val = rebuild_add_tree(ctx, expr, values, &mut 0, true);
        let fast_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = slow_idx;
        crate::expr::emit_versioned_loop_callback_deopt(ctx);
        let slow_val = rebuild_add_tree(ctx, expr, values, &mut 0, false);
        let slow_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        Ok(ctx
            .block()
            .phi(DOUBLE, &[(&fast_val, &fast_end), (&slow_val, &slow_end)]))
    })
}

/// The `+`-shaped guard for the arithmetic operators that cannot concatenate.
///
/// `-`, `*` and `/` apply ToNumeric to both operands, so unlike `+` there is
/// no string case to preserve — the whole reason `lower_guarded_numeric_add`
/// has to rebuild its tree. That makes the guard strictly simpler: test both
/// operands, do the operation inline when they are numbers, and hand the
/// original operands to the same dynamic helper otherwise.
///
/// BigInt is handled by the test rather than by a separate check: a BigInt is
/// not a Number, so `5n - 3n` and `1n - 1` both take the cold arm and keep
/// the helper's exact semantics (a BigInt result and a TypeError
/// respectively).
fn lower_guarded_numeric_arith(
    ctx: &mut FnCtx<'_>,
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
    fname: &str,
) -> Result<String> {
    let leaves = [left, right];
    let needs_test: Vec<bool> = leaves
        .iter()
        .map(|leaf| !crate::type_analysis::expr_produces_canonical_raw_f64(ctx, leaf))
        .collect();

    with_operands_rooted(ctx, &leaves, |ctx, values| {
        let mut cond: Option<String> = None;
        for (value, is_tested) in values.iter().zip(needs_test.iter()) {
            if !is_tested {
                continue;
            }
            let is_num = crate::stmt::emit_js_value_is_number(ctx, value);
            cond = Some(match cond {
                Some(prev) => ctx.block().and(I1, &prev, &is_num),
                None => is_num,
            });
        }
        let native = |ctx: &mut FnCtx<'_>, l: &str, r: &str| match op {
            BinaryOp::Sub => ctx.block().fsub(l, r),
            BinaryOp::Mul => ctx.block().fmul(l, r),
            _ => ctx.block().fdiv(l, r),
        };
        // Every leaf already vouched for: no diamond to emit.
        let Some(all_num) = cond else {
            let (l, r) = (values[0].clone(), values[1].clone());
            return Ok(native(ctx, &l, &r));
        };

        let fast_idx = ctx.new_block("guarded_arith.numeric");
        let slow_idx = ctx.new_block("guarded_arith.dynamic");
        let merge_idx = ctx.new_block("guarded_arith.merge");
        let fast_label = ctx.block_label(fast_idx);
        let slow_label = ctx.block_label(slow_idx);
        let merge_label = ctx.block_label(merge_idx);
        ctx.block().cond_br(&all_num, &fast_label, &slow_label);

        ctx.current_block = fast_idx;
        let (l, r) = (values[0].clone(), values[1].clone());
        let fast_val = native(ctx, &l, &r);
        let fast_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = slow_idx;
        crate::expr::emit_versioned_loop_callback_deopt(ctx);
        let slow_val =
            ctx.block()
                .call(DOUBLE, fname, &[(DOUBLE, &values[0]), (DOUBLE, &values[1])]);
        let slow_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        Ok(ctx
            .block()
            .phi(DOUBLE, &[(&fast_val, &fast_end), (&slow_val, &slow_end)]))
    })
}

/// `PERRY_GUARDED_ARITH=0` restores the unconditional dynamic helper for
/// `-`, `*` and `/`.
fn guarded_arith_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_GUARDED_ARITH").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Recognize `(value === null ? 0 : value) + 1` and its operand-reversed form.
///
/// Generic user-class methods currently surface as `Any` at their call sites,
/// even when a specialization has a numeric value type. That makes the common
/// counter-update idiom pay the full JS `+` helper on every hit. The expression
/// is not statically numeric — `value` may genuinely be a string — so the
/// matching add is routed through [`lower_guarded_numeric_add`], whose tag test
/// preserves concatenation in the slow arm while numbers take an inline
/// `fadd`.
///
/// Keep this deliberately structural. Requiring one repeated local, strict
/// null comparison, and numeric literals avoids turning arbitrary `Any`
/// conditionals into larger two-path add trees.
fn is_null_defaulted_local_plus_numeric_literal(left: &Expr, right: &Expr) -> bool {
    (is_strict_null_defaulted_local(left) && is_numeric_literal(right))
        || (is_numeric_literal(left) && is_strict_null_defaulted_local(right))
}

fn is_numeric_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Integer(_) | Expr::Number(_))
}

fn is_strict_null_defaulted_local(expr: &Expr) -> bool {
    let Expr::Conditional {
        condition,
        then_expr,
        else_expr,
    } = expr
    else {
        return false;
    };
    if !is_numeric_literal(then_expr) {
        return false;
    }
    let Expr::Compare {
        op: CompareOp::Eq,
        left,
        right,
    } = condition.as_ref()
    else {
        return false;
    };
    let compared_local = match (left.as_ref(), right.as_ref()) {
        (Expr::LocalGet(id), Expr::Null) | (Expr::Null, Expr::LocalGet(id)) => *id,
        _ => return false,
    };
    matches!(else_expr.as_ref(), Expr::LocalGet(id) if *id == compared_local)
}

/// The `+` tree's operand leaves, in evaluation order — a left-to-right walk,
/// so `with_operands_rooted` lowers them in the order JS evaluates them.
fn add_tree_leaves<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = expr
    {
        add_tree_leaves(left, out);
        add_tree_leaves(right, out);
    } else {
        out.push(expr);
    }
}

/// Whether a fully-dynamic `+` tree is worth one shared numeric guard.
///
/// A three-or-more-leaf tree is the common accumulator shape
/// `sum += row.x + row.y`: lowering each node independently otherwise pays
/// the dynamic add helper twice even when every runtime value is a number,
/// and one shared tag check replaces both.
///
/// Two leaves qualify as well, which the original threshold excluded on the
/// grounds that the guard "merely moves the helper behind a branch". It does
/// more than that. On the hot arm the helper call is replaced by an inline
/// `fadd`, and the operands stop going through `lower_rooted_dynamic_binary`,
/// which roots them — so a plain `s += v` accumulator was paying a call plus
/// a write barrier per add and keeping its accumulator in a shadow-frame slot
/// instead of a register. Measured on `s += v` with both operands numbers at
/// runtime but neither statically proven: 3.44 -> 0.43 ns/op against node's
/// 0.35. The cold arm still rebuilds the original tree, so a leaf that turns
/// out to be a string concatenates exactly as before.
///
/// Do not require a static numeric hint here. The important accumulator case
/// is often a captured local plus fields read from interface-shaped objects,
/// so every leaf is `Any` to codegen. The guard itself is the runtime proof;
/// its cold arm preserves the original tree and exact dynamic `+` semantics.
fn dynamic_add_tree_benefits_shared_guard(expr: &Expr) -> bool {
    if matches!(
        std::env::var("PERRY_DYNAMIC_ADD_PAIR_GUARD").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    ) {
        let mut leaves = Vec::new();
        add_tree_leaves(expr, &mut leaves);
        return leaves.len() >= 3;
    }
    let mut leaves = Vec::new();
    add_tree_leaves(expr, &mut leaves);
    leaves.len() >= 2
}

/// Rebuild the `+` tree over already-lowered leaf values, node for node, so the
/// original associativity survives. `fast` picks the inline `fadd`; otherwise
/// every node goes through the spec-`+` helper.
fn rebuild_add_tree(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
    values: &[String],
    next_leaf: &mut usize,
    fast: bool,
) -> String {
    if let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = expr
    {
        let l = rebuild_add_tree(ctx, left, values, next_leaf, fast);
        let r = rebuild_add_tree(ctx, right, values, next_leaf, fast);
        return if fast {
            ctx.block().fadd(&l, &r)
        } else {
            ctx.block().call(
                DOUBLE,
                "js_dynamic_string_or_number_add",
                &[(DOUBLE, &l), (DOUBLE, &r)],
            )
        };
    }
    let value = values[*next_leaf].clone();
    *next_leaf += 1;
    value
}

/// May the flattened `p1 + p2 + … + pN` chain be handed to
/// `js_string_concat_chain`, which formats EVERY part as a string? (#7837)
///
/// The fold reproduces the source tree `(((p1 + p2) + p3) …)` only when that
/// tree really is all-concat. Exactly one node can fail that: `p1 + p2`. If
/// either of those is genuinely a string the node concatenates, its result is
/// a string, and every later `+` concatenates too, whatever the later parts
/// hold. If neither is, the node may be a numeric ADD — and then
/// `const a: string = (42 as any), b: string = (99 as any); a + b + "x"` is
/// `"141x"` in Node while the fold prints `"4299x"`.
///
/// So the head pair needs a proof, not an annotation. A chain that fails this
/// simply falls through to the pairwise lowering, where `js_string_concat_box`
/// resolves each node from the runtime tags.
fn chain_fold_is_sound(ctx: &FnCtx<'_>, parts: &[&Expr]) -> bool {
    parts
        .iter()
        .take(2)
        .any(|p| crate::type_analysis::string_value_is_runtime_guaranteed(ctx, p))
}

/// Is this `+` the update of an accumulator whose whole chain is proven to
/// stay below 2^53 (#9363)?
///
/// Shape: `acc + <byte read>` (either operand order), where `acc` is a local
/// the trip-count analysis admitted. The analysis proved the property over
/// EVERY write to that local in the function, so recognizing the shape here is
/// only selecting which `fadd` gets the flag — it is not itself the argument.
///
/// The addend must still be checked at this site: the collector's bound covers
/// the writes it saw, and an `acc + <something else>` node inside the same
/// function is a different expression that its bound does not license.
fn reduction_add_is_reassociable(ctx: &FnCtx<'_>, left: &Expr, right: &Expr) -> bool {
    let admitted = |e: &Expr| {
        matches!(e, Expr::LocalGet(id) if ctx.native_facts.reassociable_f64_accumulators().contains(id))
    };
    let byte_read = |e: &Expr| {
        matches!(
            e,
            Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. }
        )
    };
    (admitted(left) && byte_read(right)) || (admitted(right) && byte_read(left))
}

fn lower_arithmetic_operand(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<(String, bool)> {
    // A stable-packed numeric clone has a stronger fact than the generic
    // untyped-local typed-array probe below: its preheader scanned the exact
    // range, and its emitted-IR gate proved that no call can invalidate that
    // proof. Preserve the clone's private direct load and report that no
    // residual ToNumber coercion is needed.
    if crate::stmt::stable_packed_loop::has_numeric_index_fact(ctx, expr) {
        return Ok((lower_expr(ctx, expr)?, true));
    }
    // #5497 Lever E: a representation-first Boolean local/literal is already
    // an i1. JavaScript arithmetic applies ToNumber, which is exactly an
    // unsigned i1 -> f64 conversion; boxing and calling js_number_coerce only
    // recreates information codegen already proved.
    if let Some(value) = super::try_lower_proven_boolean_to_number(ctx, expr)? {
        return Ok((value, true));
    }
    // #6884: a statically typed numeric TypedArray read is Number|undefined,
    // not an unconditional raw f64. In arithmetic context the OOB `undefined`
    // must become canonical NaN. Sink that conversion into the OOB/cold arms
    // so the in-bounds hot path remains a guard plus native load.
    // The HIR spells the same read `Uint8ArrayGet` when it already knows the
    // receiver is a `Uint8Array`/`Buffer`. Its value is a byte or (OOB)
    // `undefined`; in number context that `undefined` must become canonical
    // NaN BEFORE any `fadd`/`fmul` — IEEE arithmetic propagates the NaN-box
    // payload, so a raw `fadd acc, <undefined box>` would leave `acc` reading
    // as `undefined` (caught by the OOB differential: `a += px[17]` printed
    // `undefined`). Prefer the guarded inline load (NaN sunk into the OOB
    // arm, in-bounds path untouched); when it declines, canonicalize the
    // generic read inline with one compare + select — never a call — which
    // is exact because the only non-double this node can yield is the
    // `undefined` box.
    if let Expr::Uint8ArrayGet { index, .. } = expr {
        if is_numeric_expr(ctx, index) {
            // Deliberately NOT routed through
            // `ta_param_f64_read::try_lower_ta_f64_read_for_number_context`:
            // its checked load reads the length at `handle + 0` and the
            // elements at `handle + 16`, which is the typed-array object
            // layout. Perry's `new Uint8Array(n)` is buffer-backed (length at
            // `data - 8`), so that load treats every index as out of bounds and
            // silently yields `undefined` — measured as `checksum:0` on
            // benchmarks/suite/bench_int_arithmetic.ts. `f64_kind_from_class`
            // still maps "Uint8Array"/"Uint8ClampedArray" to a kind, so the
            // same hazard waits for any future caller that hands it one of
            // those receivers.
            //
            // `lower_expr` picks the correct tier for this node (the inline
            // buffer-view load when the receiver is a tracked view, else
            // `js_uint8array_index_get_value`); the only value it can return
            // that is not a double is the `undefined` box, so one compare +
            // select canonicalizes it exactly, with no call.
            let raw = lower_expr(ctx, expr)?;
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(&raw);
            let is_undef = blk.icmp_eq(I64, &bits, crate::nanbox::TAG_UNDEFINED_I64);
            let canonical = blk.select(I1, &is_undef, DOUBLE, "0x7FF8000000000000", &raw);
            return Ok((canonical, true));
        }
    }
    if let Expr::IndexGet { object, index } = expr {
        if let Some(value) =
            super::ta_param_f64_read::try_lower_ta_f64_read_for_number_context(ctx, object, index)?
        {
            return Ok((value, true));
        }
        // #7494: the guarded tier above declines outright for a receiver
        // tracked in `ctx.buffer_view_slots` (its own "don't shadow" comment)
        // because that tracked view owns a STRONGER-bounds native path
        // (`lower_typed_array_load`). Nothing routed a number-context read to
        // it, so a proven in-bounds buffer-view typed-array read still fell
        // through to the generic `lower_expr` tier below and picked up a
        // redundant `js_number_coerce` from the residual-coerce rule, which
        // cannot see which concrete lowering actually ran.
        if let Some(value) =
            super::buffer_access::try_lower_typed_array_f64_read_for_number_context(
                ctx, object, index,
            )?
        {
            return Ok((value, true));
        }
    }
    // Repsel Phase 4a.0 (#6904): a numeric-proven `a || b` / `a && b` /
    // `a ?? b` consumed as an arithmetic operand lowers with BOTH sides in
    // number context, so the selection is a real-double diamond (`fcmp one` +
    // phi — SimplifyCFG folds it to a `select`) instead of a boxed
    // `js_is_truthy` dispatch whose merged value then needs a site
    // `js_number_coerce`. This is the `(counts[v] || 0) + 1` histogram shape.
    //
    // Early coercion is semantics-preserving here because the consumer is an
    // arithmetic operand: every value the coerced test can misclassify
    // relative to JS truthiness under HONEST types is `undefined` (a raw-f64
    // read's hole fallback), and ToNumber(undefined) = NaN is falsy exactly
    // like `undefined`; the passed-through value is coerced by the consumer
    // regardless. `??` keeps its nullish test on the UNCOERCED left value —
    // a coerced hole (NaN) is indistinguishable from a stored NaN, but
    // `NaN ?? x` is NaN while `undefined ?? x` is `x`.
    if let Expr::Logical { op, left, right } = expr {
        if is_numeric_expr(ctx, expr) {
            let value = lower_numeric_logical_for_number_context(ctx, *op, left, right)?;
            return Ok((value, true));
        }
    }
    // repsel #7480 step 3: a tracked `arr[i].field` read inside an
    // element-shape fast clone routes to the raw-f64 lowering WITHOUT the
    // boxed-fallback test below. That test asks `receiver_class_name`, which
    // by design does not resolve an object-literal element type, so the read
    // would otherwise fall through to `lower_expr` — a generic diamond, whose
    // calls then fail the clone's call-free admission and cost the clone
    // entirely. The predicate is left alone rather than widened: this read has
    // no boxed fallback at all (the residual per-element check proves the slot
    // is a raw double before the load), so claiming one here would be a lie
    // that other consumers of that predicate would read.
    let in_element_shape_clone = matches!(expr, Expr::PropertyGet { object, property, .. }
        if crate::expr::element_shape_loop_fact_for_property_get(ctx, object, property).is_some());
    if in_element_shape_clone || expr_may_return_boxed_value_from_raw_f64_fallback(ctx, expr) {
        if let Some(value) =
            super::property_get::lower_raw_f64_class_field_get_for_number_context(ctx, expr)?
        {
            return Ok((value, true));
        }
        if let Some(value) =
            super::index_get::lower_numeric_index_get_for_number_context(ctx, expr)?
        {
            return Ok((value, true));
        }
    }
    // #5525: an untyped-receiver typed-array element read (`S[i]` with `S` an
    // `any` param — bcryptjs's Blowfish hot path) used as a non-`+` arithmetic
    // operand. Lower it as a guaranteed Number (coerce sunk into the cold slow
    // branch) so the hot per-element fast path skips the site `js_number_coerce`.
    // Only non-`+` ops reach here (`+` with an untyped operand returned via
    // `js_dynamic_string_or_number_add` above), and those always `ToNumber`
    // their operands, so early coercion is semantics-preserving.
    if let Some(value) =
        super::index_get::lower_unknown_local_index_get_for_number_context(ctx, expr)?
    {
        return Ok((value, true));
    }
    Ok((lower_expr(ctx, expr)?, false))
}

/// The shared residual-coercion rule for arithmetic operands: a lowered
/// operand still needs a `js_number_coerce` when the fallback did not already
/// coerce it AND it is either not statically numeric (booleans, `null`, …)
/// or can surface a boxed value through a raw-f64 read's cold fallback.
fn operand_needs_residual_coerce(ctx: &FnCtx<'_>, expr: &Expr, fallback_coerced: bool) -> bool {
    !fallback_coerced
        && (!is_numeric_expr(ctx, expr)
            || expr_may_return_boxed_value_from_raw_f64_fallback(ctx, expr)
            // #7773/#7506: a numeric local or compound expression initialized
            // from a declared-only field/element expression is
            // `is_numeric_expr`, but the hazard predicate above only knows how
            // to look at reads. That made both `const sum = o.x + o.y; sum *
            // scale` and `(o.x + o.y) * scale` emit a bare `fmul`. Arithmetic
            // on a NaN-box preserves the payload, so the multiply returned the
            // string unchanged. Every non-`+` arithmetic operator is a plain
            // `ToNumber` on its operands, so a coerce is the whole fix here;
            // `+` needs the concat dispatch and gets it from
            // `lower_guarded_numeric_add`. Proven raw-f64 tiers answer
            // false here, so asking about every expression keeps them exempt.
            || numeric_proof_is_declared_only(ctx, expr))
}

/// Lower an operand in number context: route through
/// [`lower_arithmetic_operand`], then apply the shared residual-coercion rule
/// — the result is ALWAYS a real (canonical) numeric double, never a
/// NaN-boxed value.
fn lower_operand_as_number(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    let (raw, fallback_coerced) = lower_arithmetic_operand(ctx, expr)?;
    if operand_needs_residual_coerce(ctx, expr, fallback_coerced) {
        Ok(ctx
            .block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &raw)]))
    } else {
        Ok(raw)
    }
}

/// Repsel Phase 4a.0: number-context lowering of a numeric-proven logical
/// selection (see the caller comment in [`lower_arithmetic_operand`]).
///
/// `&&` / `||`: the left side is lowered in number context (a real double),
/// so its truthiness test is a bare `fcmp one l, 0.0` — falsy is exactly
/// {`+0`, `-0`, NaN}, and the values that JS-truthiness could disagree on
/// (boxed `undefined` from a hole fallback) have already been coerced to NaN
/// (falsy — identical verdict to `undefined`). Both phi inputs are real
/// doubles, so the merged value feeds `fadd`/`fmul`/… with no further
/// dispatch.
///
/// `??`: the nullish test runs on the UNCOERCED left value (`bits ==
/// TAG_NULL | TAG_UNDEFINED`); the pass-through edge then coerces (only when
/// the operand carries the boxed-fallback hazard), keeping `NaN ?? x` = NaN
/// vs `undefined ?? x` = `x` byte-exact.
fn lower_numeric_logical_for_number_context(
    ctx: &mut FnCtx<'_>,
    op: LogicalOp,
    left: &Expr,
    right: &Expr,
) -> Result<String> {
    if matches!(op, LogicalOp::Coalesce) {
        let l_boxed = lower_expr(ctx, left)?;
        let is_nullish = {
            let blk = ctx.block();
            let l_bits = blk.bitcast_double_to_i64(&l_boxed);
            let is_null = blk.icmp_eq(I64, &l_bits, crate::nanbox::TAG_NULL_I64);
            let is_undef = blk.icmp_eq(I64, &l_bits, crate::nanbox::TAG_UNDEFINED_I64);
            blk.or(I1, &is_null, &is_undef)
        };
        let right_idx = ctx.new_block("numlog.coalesce.right");
        let keep_idx = ctx.new_block("numlog.coalesce.keep");
        let merge_idx = ctx.new_block("numlog.coalesce.merge");
        let right_label = ctx.block_label(right_idx);
        let keep_label = ctx.block_label(keep_idx);
        let merge_label = ctx.block_label(merge_idx);
        ctx.block().cond_br(&is_nullish, &right_label, &keep_label);

        ctx.current_block = right_idx;
        let r = lower_operand_as_number(ctx, right)?;
        let r_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = keep_idx;
        // Non-nullish left: coerce only when the operand can surface a boxed
        // value (e.g. an INT32-boxed number from a read fallback). A plain
        // proven double passes through untouched.
        let l_num = if expr_may_return_boxed_value_from_raw_f64_fallback(ctx, left) {
            ctx.block()
                .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &l_boxed)])
        } else {
            l_boxed
        };
        let keep_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        return Ok(ctx
            .block()
            .phi(DOUBLE, &[(&r, &r_end), (&l_num, &keep_end)]));
    }

    let l = lower_operand_as_number(ctx, left)?;
    let l_bool = ctx.block().fcmp("one", &l, "0.0");
    let l_end = ctx.block().label.clone();

    let then_idx = ctx.new_block("numlog.then");
    let merge_idx = ctx.new_block("numlog.merge");
    let then_label = ctx.block_label(then_idx);
    let merge_label = ctx.block_label(merge_idx);
    match op {
        // a && b: truthy left evaluates the right side; falsy left is the
        // result.
        LogicalOp::And => ctx.block().cond_br(&l_bool, &then_label, &merge_label),
        // a || b: truthy left is the result; falsy left evaluates the right.
        LogicalOp::Or => ctx.block().cond_br(&l_bool, &merge_label, &then_label),
        LogicalOp::Coalesce => unreachable!("handled above"),
    }

    ctx.current_block = then_idx;
    let r = lower_operand_as_number(ctx, right)?;
    let r_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(DOUBLE, &[(&l, &l_end), (&r, &r_end)]))
}

fn small_bigint_literal_value(expr: &Expr) -> Option<i64> {
    let Expr::BigInt(raw) = expr else {
        return None;
    };
    let normalized = raw.replace('_', "");
    let s = normalized.strip_suffix('n').unwrap_or(&normalized);
    let (negative, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if digits.is_empty() {
        return None;
    }
    let (radix, digits) = if let Some(rest) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, rest)
    } else if let Some(rest) = digits
        .strip_prefix("0o")
        .or_else(|| digits.strip_prefix("0O"))
    {
        (8, rest)
    } else if let Some(rest) = digits
        .strip_prefix("0b")
        .or_else(|| digits.strip_prefix("0B"))
    {
        (2, rest)
    } else {
        (10, digits)
    };
    if digits.is_empty() {
        return None;
    }
    let magnitude = i128::from_str_radix(digits, radix).ok()?;
    let value = if negative { -magnitude } else { magnitude };
    i64::try_from(value).ok()
}

fn small_bigint_native_op(op: BinaryOp) -> Option<(&'static str, &'static str)> {
    match op {
        BinaryOp::Add => Some(("add", "js_dynamic_add")),
        BinaryOp::Sub => Some(("sub", "js_dynamic_sub")),
        BinaryOp::Mul => Some(("mul", "js_dynamic_mul")),
        _ => None,
    }
}

/// Six bitwise/shift ops whose result is always `ToInt32`/`ToUint32`-wrapped
/// (a plain JS Number). These are the ops the non-BigInt inline fast path
/// covers; the arithmetic ops in the same dynamic-helper bail (`Mul`/`Div`/
/// `Mod`/`Sub`/`Pow`) are deliberately excluded.
fn is_bitwise_op(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::UShr
    )
}

/// `PERRY_INLINE_NONBIGINT_BITWISE` fast-path gate. Enabled by default;
/// `=0`/`off`/`false` reverts to the BigInt-aware `js_dynamic_bit*` runtime
/// call for every non-statically-numeric bitwise operand (pre-fix behavior).
/// Kept as an env flag for A/B bisection, consistent with the sibling codegen
/// fast paths (the object cache keys this var so a warm cache can't serve an
/// object built under the other setting).
fn inline_nonbigint_bitwise_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_INLINE_NONBIGINT_BITWISE").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

fn bigint_dynamic_helper(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "js_dynamic_add",
        BinaryOp::Sub => "js_dynamic_sub",
        BinaryOp::Mul => "js_dynamic_mul",
        BinaryOp::Div => "js_dynamic_div",
        BinaryOp::Mod => "js_dynamic_mod",
        BinaryOp::BitAnd => "js_dynamic_bitand",
        BinaryOp::BitOr => "js_dynamic_bitor",
        BinaryOp::BitXor => "js_dynamic_bitxor",
        BinaryOp::Shl => "js_dynamic_shl",
        BinaryOp::Shr => "js_dynamic_shr",
        BinaryOp::Pow => "js_dynamic_pow",
        BinaryOp::UShr => "js_dynamic_ushr",
    }
}

fn record_small_bigint_rejection(
    ctx: &mut FnCtx<'_>,
    reason: &'static str,
    fallback_helper: &'static str,
) {
    let lowered = LoweredValue::js_value("0.0");
    ctx.record_lowered_value_with_access_mode(
        "BigIntSmallBinaryRejected",
        None,
        "small_bigint.literal_binary_rejected",
        &lowered,
        None,
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(MaterializationReason::RuntimeApi),
        false,
        false,
        vec![
            format!("small_bigint_rejected={reason}"),
            format!("fallback={fallback_helper}"),
            "boxed_at=generic_bigint_dynamic_helper".to_string(),
        ],
    );
}

fn try_lower_small_bigint_literal_binary(
    ctx: &mut FnCtx<'_>,
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
) -> Option<String> {
    let (native_op, fallback_helper) = small_bigint_native_op(op)?;
    let Some(left_i64) = small_bigint_literal_value(left) else {
        record_small_bigint_rejection(ctx, "requires_left_i64_literal", fallback_helper);
        return None;
    };
    let Some(right_i64) = small_bigint_literal_value(right) else {
        record_small_bigint_rejection(ctx, "requires_right_i64_literal", fallback_helper);
        return None;
    };

    let left_const = left_i64.to_string();
    let right_const = right_i64.to_string();
    let result_i128 = {
        let blk = ctx.block();
        let left_wide = blk.sext(I64, &left_const, I128);
        let right_wide = blk.sext(I64, &right_const, I128);
        match op {
            BinaryOp::Add => blk.add(I128, &left_wide, &right_wide),
            BinaryOp::Sub => blk.sub(I128, &left_wide, &right_wide),
            BinaryOp::Mul => blk.mul(I128, &left_wide, &right_wide),
            _ => return None,
        }
    };
    let lowered = LoweredValue::small_bigint(result_i128.clone());
    ctx.record_lowered_value(
        "BigIntSmallBinary",
        None,
        "small_bigint.literal_binary_i128",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        vec![
            "proof=both_operands_bigint_literals_fit_i64".to_string(),
            format!("native_op=i128_{native_op}"),
            "public_semantics=materialize_bigint_object_before_js_boundary".to_string(),
        ],
    );
    let ptr = {
        let blk = ctx.block();
        let lo = blk.trunc(I128, &result_i128, I64);
        let hi_wide = blk.ashr(I128, &result_i128, "64");
        let hi = blk.trunc(I128, &hi_wide, I64);
        blk.call(I64, "js_bigint_from_i128_parts", &[(I64, &lo), (I64, &hi)])
    };
    Some(materialize_small_bigint_pointer_to_js_value(
        ctx,
        &ptr,
        MaterializationReason::RuntimeApi,
    ))
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Binary { op, left, right } => {
            // A proven-u31 index turns the canonical 32-bit ECS bitset test
            // into one guarded Uint32Array load. Match before the erased
            // receiver makes the ordinary BitAnd arm choose its fully dynamic
            // BigInt-capable lowering; every guard miss retains that lowering
            // in the peephole's slow arm.
            if let Some(value) = super::bitset_test::try_lower_u32_bitset_test(ctx, expr)? {
                return Ok(value);
            }
            if matches!(op, BinaryOp::Add) {
                // Use the stricter `is_definitely_string_expr` check for
                // the string-concat fast path. A union type `string|number`
                // that happens to contain a number at runtime would get
                // misrouted through lower_string_coerce_concat, which
                // treats the operand as a string pointer (bitcast + mask)
                // and reads garbage. The numeric Add path below handles
                // narrowed-number unions correctly via js_number_coerce.
                let l_is_str = crate::type_analysis::is_definitely_string_expr(ctx, left);
                let r_is_str = crate::type_analysis::is_definitely_string_expr(ctx, right);

                // N-way string concat fold (v0.5.771): when this is a
                // chain of `a + b + c + ...` where every Add node has at
                // least one statically-string operand, flatten the entire
                // left-spine and emit a single `js_string_concat_chain`
                // call. Saves N-1 intermediate StringHeader allocations
                // per row in mixed-type CSV / log-line / template
                // patterns. Only fires for chains of 3+ parts; smaller
                // shapes go through the existing pairwise paths.
                if l_is_str && r_is_str {
                    if let Some(parts) = flatten_string_add_chain(ctx, left, right) {
                        if parts.len() >= 3 && chain_fold_is_sound(ctx, &parts) {
                            return lower_string_concat_chain(ctx, &parts);
                        }
                    }
                }

                // The pairwise concat — and ONLY the pairwise concat — accepts a
                // DECLARED `string` as well as a structurally-proven one, so
                // `"shape:" + this.tag` and `prefix + r.kind` stop paying
                // `js_dynamic_string_or_number_add`'s scope + four roots + two
                // `ToPrimitive`s to rediscover what the declarations said.
                //
                // Sound for a LYING declaration, not merely unlikely to meet
                // one: this arm emits `js_string_concat_box`, which
                // tag-dispatches both operands and forwards any non-string pair
                // to `js_dynamic_string_or_number_add` — so string+string,
                // string+number and number+number all return exactly what the
                // dynamic path returns. See `is_declared_string_expr` for why
                // the chain fold above and the one-sided arm below must keep
                // the strict predicate.
                if crate::type_analysis::is_declared_string_expr(ctx, left)
                    && crate::type_analysis::is_declared_string_expr(ctx, right)
                {
                    return lower_string_concat(ctx, left, right);
                }
                if l_is_str || r_is_str {
                    let other_known_primitive = if l_is_str {
                        crate::type_analysis::is_numeric_expr(ctx, right)
                            || is_bigint_expr(ctx, right)
                            || is_bool_expr(ctx, right)
                            || matches!(
                                right.as_ref(),
                                Expr::LocalGet(id)
                                    if matches!(
                                        ctx.local_type_hint(id),
                                        Some(HirType::Number | HirType::Int32)
                                    )
                            )
                    } else {
                        crate::type_analysis::is_numeric_expr(ctx, left)
                            || is_bigint_expr(ctx, left)
                            || is_bool_expr(ctx, left)
                            || matches!(
                                left.as_ref(),
                                Expr::LocalGet(id)
                                    if matches!(
                                        ctx.local_type_hint(id),
                                        Some(HirType::Number | HirType::Int32)
                                    )
                            )
                    };
                    if other_known_primitive {
                        return lower_string_coerce_concat(ctx, left, right, l_is_str, r_is_str);
                    }
                    return lower_rooted_dynamic_binary(
                        ctx,
                        "js_dynamic_string_or_number_add",
                        left,
                        right,
                    );
                }
                if is_bigint_expr(ctx, left) && is_bigint_expr(ctx, right) {
                    if let Some(value) = try_lower_small_bigint_literal_binary(
                        ctx,
                        *op,
                        left.as_ref(),
                        right.as_ref(),
                    ) {
                        return Ok(value);
                    }
                    return lower_rooted_dynamic_binary(ctx, "js_dynamic_add", left, right);
                }
                // Refs #486: neither operand is statically known. Per JS
                // spec for `+`, if EITHER side is a string at runtime, the
                // result is string concatenation; otherwise numeric add
                // (or BigInt add when bigint is involved). Pre-fix, the
                // numeric-fallback path below called js_number_coerce on
                // both sides — turning `"c" + ""` into `NaN + 0 = NaN` for
                // any string operand whose type wasn't statically inferred.
                // Hono's `Node.buildRegExpStr` does `k + c.buildRegExpStr()`
                // inside a for-of loop over `Object.keys(...)` results;
                // both operands lower as plain f64s with type Any, the
                // string-concat fast path didn't fire, and every recursive
                // step poisoned the result. Dispatch through the runtime
                // helper that checks NaN-box tags: STRING_TAG / SHORT_STRING_TAG
                // → string concat, BIGINT → bigint add, otherwise numeric.
                let both_numeric = crate::type_analysis::is_numeric_expr(ctx, left)
                    && crate::type_analysis::is_numeric_expr(ctx, right);
                // #8607: a generic registry lookup leaves the counter value
                // typed as `Any`, so `(prev === null ? 0 : prev) + 1` used to
                // call the full JS add dispatcher on every pipeline stage.
                // Keep its dynamic semantics in a cold arm and let the common
                // numeric value take the same guarded fadd used for violable
                // declared-number reads.
                if is_null_defaulted_local_plus_numeric_literal(left, right) {
                    return lower_guarded_numeric_add(ctx, expr);
                }
                // `+` is the one arithmetic operator that must distinguish
                // numeric addition from string concatenation before lowering
                // its operands. Admit native-i1 Booleans only when the other
                // side is another proven Boolean or a canonical raw f64. A
                // declared-only Number/Boolean stays on the dynamic helper so
                // `as any` can still turn the operation into concatenation.
                let left_bool = super::can_lower_proven_boolean_to_number(ctx, left);
                let right_bool = super::can_lower_proven_boolean_to_number(ctx, right);
                let boolean_numeric_add = (left_bool || right_bool)
                    && (left_bool
                        || crate::type_analysis::expr_produces_canonical_raw_f64(ctx, left))
                    && (right_bool
                        || crate::type_analysis::expr_produces_canonical_raw_f64(ctx, right));
                let materialization_hazard =
                    add_operands_have_pod_materialization_hazard(ctx, left, right);
                if !(both_numeric || boolean_numeric_add) || materialization_hazard {
                    if dynamic_add_tree_benefits_shared_guard(expr) && !materialization_hazard {
                        return lower_guarded_numeric_add(ctx, expr);
                    }
                    return lower_rooted_dynamic_binary(
                        ctx,
                        "js_dynamic_string_or_number_add",
                        left,
                        right,
                    );
                }
                // Both sides are statically numeric — but "statically" can mean
                // "an annotation said so", and annotations are not enforced
                // (#7773, #7776). Re-check the tag at runtime rather than
                // emitting a bare `fadd` on a value that may be NaN-boxed.
                if numeric_proof_is_declared_only(ctx, left)
                    || numeric_proof_is_declared_only(ctx, right)
                {
                    return lower_guarded_numeric_add(ctx, expr);
                }
            }
            // BigInt arithmetic fast path. NaN-tagged bigints compare
            // unordered under `fadd`/`fsub`/`fmul`/`fdiv`/`frem` (the
            // tag bits make the f64 a NaN), so the default numeric path
            // returns `NaN` for `5n + 3n` and friends. When either side
            // is statically bigint-typed we dispatch to the runtime's
            // dynamic helpers — they unbox, call `js_bigint_<op>`, and
            // re-box with BIGINT_TAG. These helpers also tolerate
            // mixed bigint/int32 operands (they upcast to bigint), so
            // `n * 10n` where `n` is a bigint loop accumulator works
            // even when the numeric literal side isn't a bigint. Add is
            // in here too — `bigint + bigint` is arithmetic, not string
            // concat (the `is_definitely_string_expr` check above
            // already ruled out the string case). Closes GH #33.
            if is_bigint_expr(ctx, left) || is_bigint_expr(ctx, right) {
                let fname = bigint_dynamic_helper(*op);
                if let Some(value) =
                    try_lower_small_bigint_literal_binary(ctx, *op, left.as_ref(), right.as_ref())
                {
                    return Ok(value);
                }
                return lower_rooted_dynamic_binary(ctx, fname, left, right);
            }
            // A non-primitive operand may `ToNumeric` to a BigInt at runtime
            // (`Object(1n)`, or an object with a BigInt-returning
            // `Symbol.toPrimitive`/`valueOf`). The numeric fast path below
            // `js_number_coerce`s both sides — collapsing a boxed BigInt to a
            // Number and silently producing a Number result instead of the
            // spec-mandated TypeError (mixed) or BigInt (both-bigint). Route
            // such operands through the dynamic helper, which runs full
            // `ToNumeric` (test262 `bigint-and-number` / `bigint-non-primitive`
            // for the object cases). Only the arithmetic/bitwise ops with a
            // dynamic helper are affected; the common all-numeric shapes (both
            // operands statically numeric/bool) keep the fast path untouched.
            if matches!(
                op,
                BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
                    | BinaryOp::UShr
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Sub
                    | BinaryOp::Pow
            ) {
                let l_prim =
                    crate::type_analysis::is_numeric_expr(ctx, left) || is_bool_expr(ctx, left);
                let r_prim =
                    crate::type_analysis::is_numeric_expr(ctx, right) || is_bool_expr(ctx, right);
                if !(l_prim && r_prim) {
                    // Non-BigInt inline fast path (the bcryptjs `_encipher`
                    // Feistel lever): for the six BITWISE ops, when BOTH
                    // operands are provably-not-BigInt we skip the dynamic
                    // helper and fall through to the inline `ToInt32 <op>
                    // ToInt32 + sitofp` lowering below. That path already
                    // picks the NaN-safe guarded `toint32_wrap` for any
                    // operand not proven finite (e.g. an OOB typed-array read
                    // → `undefined`/NaN), and `js_number_coerce`s non-numeric
                    // operands, so semantics are preserved. We keep the
                    // dynamic-helper bail whenever an operand *could* be a
                    // BigInt (so `bigint <op> number` still throws and
                    // `bigint <op> bigint` still computes a BigInt), and for
                    // the arithmetic ops (`Mul`/`Div`/`Mod`/`Sub`/`Pow`),
                    // which are out of scope for this fast path.
                    let inline_bitwise = is_bitwise_op(*op)
                        && inline_nonbigint_bitwise_enabled()
                        && crate::type_analysis::is_provably_not_bigint(ctx, left)
                        && crate::type_analysis::is_provably_not_bigint(ctx, right);
                    if !inline_bitwise {
                        // #6951: the dynamic helper runs ToNumeric on both
                        // operands, so a pointer-bearing left operand must
                        // survive the right operand's evaluation.
                        let fname = bigint_dynamic_helper(*op);
                        // `-`, `*` and `/` take the same guarded diamond `+`
                        // got in #9159. The bail above is reached whenever an
                        // operand MIGHT be a BigInt, which for an unproven
                        // operand is always — so `s -= v` paid a call per
                        // operation while `s += v` did not. The guard needs no
                        // proof: a BigInt is not a Number, so it fails the test
                        // and the cold arm runs this same helper.
                        if guarded_arith_enabled()
                            && matches!(op, BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div)
                        {
                            return lower_guarded_numeric_arith(ctx, *op, left, right, fname);
                        }
                        return lower_rooted_dynamic_binary(ctx, fname, left, right);
                    }
                }
            }
            // Fast path: `<integer-valued> % <integer literal>` (the
            // factorial / `i % 1000` loop shape). `frem double` lowers
            // to a libm `fmod()` call on ARM — no hardware instruction
            // — at ~15ns per iteration. Emitting `fptosi → srem →
            // sitofp` lets LLVM's SCEV hoist the float↔int conversions
            // out of the loop and replace the div with a reciprocal-
            // multiplication trick. On the factorial benchmark this
            // takes the inner loop from 1550ms → ~150ms.
            //
            // Safety: both operands must be provably integer-valued.
            // A fractional LHS would lose its fraction bits through
            // fptosi, producing the wrong result. `is_integer_valued_expr`
            // only returns true when we can prove the value is a whole
            // number (integer literals, integer loop counters, or nested
            // integer arithmetic). A zero RHS cannot use this unconditional
            // path because srem(x,0) is UB in LLVM (on ARM the CPU silently
            // gives 0, but JS requires NaN for any x % 0). Everything not
            // proven here reaches the guarded i32 path and its `frem`
            // fallback below.
            if matches!(op, BinaryOp::Mod)
                && crate::type_analysis::is_integer_valued_expr(ctx, left)
                && matches!(right.as_ref(), Expr::Integer(divisor) if *divisor != 0)
            {
                let l_raw = lower_expr(ctx, left)?;
                let r_raw = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let li = blk.fptosi(DOUBLE, &l_raw, I64);
                let ri = blk.fptosi(DOUBLE, &r_raw, I64);
                let m = blk.srem(I64, &li, &ri);
                // IEEE 754: when the integer remainder is 0 and the
                // dividend was negative, the result must be -0.0.
                // srem gives 0i64 → sitofp always produces +0.0,
                // so correct: if m==0 && l<0 → fneg(0.0) = -0.0.
                let result_f = blk.sitofp(I64, &m, DOUBLE);
                let m_is_zero = blk.icmp_eq(I64, &m, "0");
                let l_neg = blk.fcmp("olt", &l_raw, "0.0");
                let need_neg = blk.and(I1, &m_is_zero, &l_neg);
                let neg_result = blk.fneg(&result_f);
                return Ok(blk.select(I1, &need_neg, DOUBLE, &neg_result, &result_f));
            }

            let (l_raw, l_fallback_coerced) = lower_arithmetic_operand(ctx, left)?;
            let (r_raw, r_fallback_coerced) = lower_arithmetic_operand(ctx, right)?;
            // Coerce non-numeric operands to numbers for arithmetic.
            // JS: `true + true = 2`, `null + 1 = 1`, etc. Without
            // this, fadd on NaN-tagged booleans propagates the NaN
            // payload instead of computing 1.0 + 1.0 = 2.0.
            let l_needs_coerce = operand_needs_residual_coerce(ctx, left, l_fallback_coerced);
            let r_needs_coerce = operand_needs_residual_coerce(ctx, right, r_fallback_coerced);
            let l = if l_needs_coerce {
                ctx.block()
                    .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &l_raw)])
            } else {
                l_raw
            };
            let r = if r_needs_coerce {
                ctx.block()
                    .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &r_raw)])
            } else {
                r_raw
            };
            let v = match op {
                BinaryOp::Add => {
                    // #9363: a proven byte-read reduction may reassociate, so
                    // LLVM can split the serial dependency chain into parallel
                    // partial sums (and vectorize it). The proof is the
                    // collector's, not this site's — see
                    // `collectors/loop_bounded_i32.rs::AccumulatorMode`.
                    let reassoc = reduction_add_is_reassociable(ctx, left, right);
                    let blk = ctx.block();
                    if reassoc {
                        blk.fadd_reassoc(&l, &r)
                    } else {
                        blk.fadd(&l, &r)
                    }
                }
                BinaryOp::Sub => {
                    let blk = ctx.block();
                    blk.fsub(&l, &r)
                }
                BinaryOp::Mul => {
                    let blk = ctx.block();
                    blk.fmul(&l, &r)
                }
                BinaryOp::Div => {
                    let blk = ctx.block();
                    blk.fdiv(&l, &r)
                }
                BinaryOp::Mod => lower_checked_i32_modulo(ctx, &l, &r),
                BinaryOp::Pow => {
                    ctx.block()
                        .call(DOUBLE, "js_math_pow", &[(DOUBLE, &l), (DOUBLE, &r)])
                }
                // Bitwise ops: use toint32_fast (skip NaN/Inf guard) when
                // operands are known-finite from integer analysis.
                //
                // `x | 0` and `x >>> 0` where x is known-finite: the op
                // is just a ToInt32/ToUint32 coercion. When x comes from
                // the integer path (already finite), skip the toint32
                // entirely — just fptosi + sitofp (identity for in-range
                // values, LLVM eliminates via instcombine).
                BinaryOp::BitOr
                    if matches!(right.as_ref(), Expr::Integer(0))
                        && is_known_i32_range(ctx, left) =>
                {
                    let blk = ctx.block();
                    let li = blk.toint32_fast(&l);
                    blk.sitofp(I32, &li, DOUBLE)
                }
                BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr => {
                    let l_safe = is_known_i32_range(ctx, left);
                    let r_safe = is_known_i32_range(ctx, right);
                    let blk = ctx.block();
                    let li = if l_safe {
                        blk.toint32_fast(&l)
                    } else {
                        blk.toint32_wrap(&l)
                    };
                    let ri = if r_safe {
                        blk.toint32_fast(&r)
                    } else {
                        blk.toint32_wrap(&r)
                    };
                    let v = match op {
                        BinaryOp::BitAnd => blk.and(I32, &li, &ri),
                        BinaryOp::BitOr => blk.or(I32, &li, &ri),
                        BinaryOp::BitXor => blk.xor(I32, &li, &ri),
                        BinaryOp::Shl => blk.shl(I32, &li, &ri),
                        BinaryOp::Shr => blk.ashr(I32, &li, &ri),
                        _ => unreachable!(),
                    };
                    blk.sitofp(I32, &v, DOUBLE)
                }
                BinaryOp::UShr
                    if matches!(right.as_ref(), Expr::Integer(0))
                        && is_known_i32_range(ctx, left) =>
                {
                    let blk = ctx.block();
                    let li = blk.toint32_fast(&l);
                    blk.uitofp(I32, &li, DOUBLE)
                }
                BinaryOp::UShr => {
                    let l_safe = is_known_i32_range(ctx, left);
                    let r_safe = is_known_i32_range(ctx, right);
                    let blk = ctx.block();
                    let li = if l_safe {
                        blk.toint32_fast(&l)
                    } else {
                        blk.toint32_wrap(&l)
                    };
                    let ri = if r_safe {
                        blk.toint32_fast(&r)
                    } else {
                        blk.toint32_wrap(&r)
                    };
                    let v = blk.lshr(I32, &li, &ri);
                    blk.uitofp(I32, &v, DOUBLE)
                }
            };
            Ok(v)
        }

        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
