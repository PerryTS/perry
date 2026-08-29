//! `Stmt::Switch` lowering.

use super::*;
use crate::types::{DOUBLE, I1, I32, I64};

/// Minimum number of UNIQUE numeric-literal case values before the
/// binary-search dispatch tree replaces the linear test tower. Below this the
/// tower's straight-line compare chain is already cheap and the tree's
/// canonicalization prologue isn't worth its extra blocks.
const NUMERIC_TREE_MIN_CASES: usize = 8;

/// `switch (disc) { case A: ...; break; case B: ...; default: ... }`
/// lowering. Each case gets a (test, body) block pair; bodies fall
/// through to the next body block (not the next test) to honor JS
/// fall-through. The default body is positioned wherever the default
/// case appears in source order. `break` inside a case branches to
/// the exit block via the `loop_targets` mechanism.
///
/// We don't use LLVM's `switch` instruction because the discriminant
/// is a NaN-boxed double whose equality semantics differ from i32
/// switch (NaN != NaN). The if-tower lowering uses fcmp oeq for each
/// test which yields the right semantics.
///
/// Dense all-numeric switches take a faster path: when EVERY case test is a
/// compile-time integer-valued numeric literal and there are at least
/// [`NUMERIC_TREE_MIN_CASES`] unique values, the linear tower is replaced by
/// a balanced binary-search tree over the sorted case values — O(log n)
/// `fcmp` branches per dispatch instead of O(n) `js_switch_strict_equals`
/// calls (babel-style AST/token-kind dispatch switches have 50–100+ arms).
/// Only the DISPATCH changes: bodies keep source order and fall-through, the
/// default body is still the no-match target, and duplicate case values still
/// resolve to the first (source-order) clause. See
/// [`numeric_literal_dispatch_plan`] for why this is unobservable.
pub(crate) fn lower_switch(
    ctx: &mut FnCtx<'_>,
    discriminant: &perry_hir::Expr,
    cases: &[perry_hir::SwitchCase],
) -> Result<()> {
    let dv = lower_expr(ctx, discriminant)?;

    let tree_plan = numeric_literal_dispatch_plan(cases);

    // Allocate body blocks (and, for the tower lowering, test blocks) for
    // every case up front so we can wire up the fall-through edges before
    // each block is filled in. The tree lowering creates its dispatch blocks
    // on the fly instead of per-case test blocks (an unfilled block would
    // have no terminator and fail LLVM verification).
    let mut test_blocks: Vec<usize> = Vec::with_capacity(cases.len());
    let mut body_blocks: Vec<usize> = Vec::with_capacity(cases.len());
    for (i, case) in cases.iter().enumerate() {
        if tree_plan.is_none() {
            let test_name = if case.test.is_some() {
                format!("switch.test{}", i)
            } else {
                format!("switch.default_test{}", i)
            };
            test_blocks.push(ctx.new_block(&test_name));
        }
        body_blocks.push(ctx.new_block(&format!("switch.body{}", i)));
    }
    let exit_idx = ctx.new_block("switch.exit");
    let exit_label = ctx.block_label(exit_idx);

    if cases.is_empty() {
        ctx.block().br(&exit_label);
        ctx.current_block = exit_idx;
        return Ok(());
    }

    // Find the default case index, if any. The "no case matched" target
    // is the default's body block; if there's no default, it is exit.
    let default_idx = cases.iter().position(|c| c.test.is_none());
    let no_match_target_label = match default_idx {
        Some(i) => ctx.block_label(body_blocks[i]),
        None => exit_label.clone(),
    };

    match &tree_plan {
        Some(plan) => {
            // Dense numeric switch: canonicalize the discriminant once and
            // dispatch through a balanced comparison tree. Case tests are
            // all literals, so skipping their (side-effect-free) in-order
            // evaluation is unobservable.
            emit_numeric_tree_dispatch(ctx, &dv, plan, &body_blocks, &no_match_target_label);
        }
        None => {
            // Branch from the discriminant block into the first *case* test.
            // A leading `default:` is skipped — its body only runs when no
            // case test anywhere in the block matches.
            let first_case_test = cases.iter().position(|c| c.test.is_some());
            match first_case_test {
                Some(i) => {
                    let first_test_label = ctx.block_label(test_blocks[i]);
                    ctx.block().br(&first_test_label);
                }
                None => {
                    // Only a default clause: run it unconditionally.
                    ctx.block().br(&no_match_target_label);
                }
            }
        }
    }

    // Push break target. A switch has NO continue target — the cont slot is an
    // empty sentinel so `Stmt::Continue` skips this entry and resolves to the
    // innermost enclosing LOOP. (It previously pushed the exit label for both,
    // so `continue` inside a switch-in-loop branched to the switch EXIT — i.e.
    // it behaved like `break` and fell into the post-switch tail. react-server-
    // dom's flight row parser is a for-loop state machine whose case arms end
    // in `continue`; every row-boundary step executed the row-commit tail with
    // a stale slice index, mis-framing every RSC row — #5989.)
    ctx.loop_targets
        .push((String::new(), exit_label.clone(), ctx.try_depth));

    // If this switch carries a pending label (from an enclosing
    // `a: switch (...)`), register it so `break a;` resolves to THIS
    // switch's exit even when the break sits inside a *nested* switch.
    // Without this, `LabeledBreak` falls back to the innermost
    // `loop_targets` entry — the nested switch's own exit — so the break
    // escapes only the inner switch and execution falls through into the
    // outer switch's remaining code (e.g. react-reconciler's
    // `switch (type.$$typeof) { case CTX: t = 10; break a; }`, where the
    // case body's `break a` was landing in the invalid-element tail and
    // every Context.Provider became fiber tag 29 → children dropped).
    let consumed_labels = std::mem::take(&mut ctx.pending_labels);
    for lbl in &consumed_labels {
        ctx.label_targets.insert(
            lbl.clone(),
            (exit_label.clone(), exit_label.clone(), ctx.try_depth),
        );
    }

    // Compile each test block (tower lowering only — the tree already emitted
    // its dispatch blocks above). Each test compares dv against the case
    // expression with fcmp oeq, jumps to the body on match, otherwise
    // jumps to the next *case* test (or to no_match_target if this is the
    // last). The default clause is NOT part of the test chain: per spec
    // CaseBlockEvaluation, every case test — including ones written
    // *after* `default:` — is tried first, and the default body only
    // runs when no case matched. A non-match at the default's source
    // position must therefore skip over it to the next case test.
    if tree_plan.is_none() {
        for (i, case) in cases.iter().enumerate() {
            ctx.current_block = test_blocks[i];
            let body_label = ctx.block_label(body_blocks[i]);
            let next_case_test = ((i + 1)..cases.len()).find(|&j| cases[j].test.is_some());
            let next_label = match next_case_test {
                Some(j) => ctx.block_label(test_blocks[j]),
                None => no_match_target_label.clone(),
            };

            if let Some(test_expr) = case.test.as_ref() {
                let cv = lower_expr(ctx, test_expr)?;
                // CaseClauseIsSelected is strict equality (`===`). One runtime
                // helper covers every value-kind correctly: string content
                // compare (heap + SSO), IEEE numeric compare (NaN never
                // matches, -0 == +0, int32-boxed == raw double), and bit
                // identity for objects/null/undefined/booleans. The previous
                // two-path lowering (js_get_string_pointer_unified +
                // js_string_equals, raw bit compare otherwise) made
                // `switch (1)` match `case '1'` through the unified getter's
                // number→string property-key coercion (S12.11_A1_T2) and
                // `switch (NaN)` match `case NaN` through bit equality.
                let blk = ctx.block();
                let i32_eq = blk.call(
                    crate::types::I32,
                    "js_switch_strict_equals",
                    &[(crate::types::DOUBLE, &dv), (crate::types::DOUBLE, &cv)],
                );
                let cmp = blk.icmp_ne(crate::types::I32, &i32_eq, "0");
                blk.cond_br(&cmp, &body_label, &next_label);
            } else {
                // Default case test block: unconditional jump to its body.
                ctx.block().br(&body_label);
            }
        }
    }

    // Compile each body block. Bodies fall through to the next body
    // (NOT the next test) unless terminated by `break`/`return`/etc.
    for (i, case) in cases.iter().enumerate() {
        ctx.current_block = body_blocks[i];
        lower_stmts(ctx, &case.body)?;
        if !ctx.block().is_terminated() {
            let next_body_label = if i + 1 < body_blocks.len() {
                ctx.block_label(body_blocks[i + 1])
            } else {
                exit_label.clone()
            };
            ctx.block().br(&next_body_label);
        }
    }

    ctx.loop_targets.pop();
    ctx.current_block = exit_idx;
    Ok(())
}

/// Decide whether this switch qualifies for the binary-search dispatch tree
/// and, if so, return its plan: the unique case values sorted ascending, each
/// paired with the index of the FIRST case clause carrying that value.
///
/// Qualification rules — each one guards an observable-semantics requirement:
///
/// - Every non-default case test must be `Expr::Number` / `Expr::Integer`.
///   JS evaluates case-test expressions in source order until one matches
///   (spec CaseBlockEvaluation), so reordering tests into a tree is only
///   legal when every test is a side-effect-free literal whose evaluation
///   cannot be observed. (`case -3:` qualifies too: HIR lowering folds unary
///   minus over numeric literals — including `-0` → `Number(-0.0)` — so it
///   arrives here as a literal, see lower_expr/arm_unary.rs.)
/// - Values must be finite and integer-valued. This keeps every tree constant
///   well clear of the subnormal range, where a raw module-slot object
///   pointer's bit pattern (top16 == 0, see `normalize_raw_object_bits`)
///   decodes to a denormal double: such a discriminant flows through the
///   tree's numeric path as that denormal, which can never `fcmp oeq` an
///   integer-valued constant — same no-match verdict the tower's
///   `js_switch_strict_equals` (which re-tags the pointer) produces. NaN and
///   ±Infinity literals are rejected with the same check for simplicity.
/// - Duplicate values keep the FIRST clause (stable sort by value preserves
///   source order among equals; dedup retains the leading element), matching
///   the tower's first-test-wins order. `Expr::Integer` converts via
///   `as f64` — exactly how `literals_vars::lower` materializes it — so two
///   i64 literals that collapse to one double here also compare equal at
///   runtime under the tower.
/// - The default clause (test == None) is skipped: it is not part of the
///   test chain; the tree's no-match edge targets its body.
fn numeric_literal_dispatch_plan(cases: &[perry_hir::SwitchCase]) -> Option<Vec<(f64, usize)>> {
    use perry_hir::Expr;
    let mut vals: Vec<(f64, usize)> = Vec::with_capacity(cases.len());
    for (i, case) in cases.iter().enumerate() {
        let Some(test) = case.test.as_ref() else {
            continue;
        };
        let v = match test {
            Expr::Number(f) => *f,
            Expr::Integer(n) => *n as f64,
            _ => return None,
        };
        if !v.is_finite() || v.fract() != 0.0 {
            return None;
        }
        vals.push((v, i));
    }
    vals.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .expect("case values are finite (checked above)")
    });
    vals.dedup_by(|later, first| later.0 == first.0);
    if vals.len() < NUMERIC_TREE_MIN_CASES {
        return None;
    }
    Some(vals)
}

/// Emit the O(log n) dispatch for a qualifying numeric switch: canonicalize
/// the NaN-boxed discriminant to a raw double once, then walk a balanced
/// binary-search tree of `fcmp oeq`/`fcmp olt` branches over the sorted case
/// values.
///
/// Canonicalization mirrors `js_switch_strict_equals`' numeric arm exactly:
///
/// - int32-boxed (`(bits & !INT32_MASK) == INT32_TAG`) → sign-extend the low
///   32 bits and `sitofp` (so int32-boxed 7 matches `case 7:`).
/// - any other value whose top-16 tag lies outside Perry's tag band
///   (`SHORT_STRING_TAG..=STRING_TAG`) is already a raw double — including
///   real NaNs, which then fail every ordered `fcmp` in the tree and fall out
///   at the no-match edge, exactly like NaN != NaN in the helper.
/// - strings/bools/undefined/null/pointers (tagged, non-int32) can never
///   strictly equal a number: branch straight to no-match.
fn emit_numeric_tree_dispatch(
    ctx: &mut FnCtx<'_>,
    dv: &str,
    plan: &[(f64, usize)],
    body_blocks: &[usize],
    no_match_label: &str,
) {
    let bits = ctx.block().bitcast_double_to_i64(dv);
    let int32_band = ctx.block().and(
        I64,
        &bits,
        &crate::nanbox::i64_literal(!crate::nanbox::INT32_MASK),
    );
    let is_int32 = ctx.block().icmp_eq(
        I64,
        &int32_band,
        &crate::nanbox::i64_literal(crate::nanbox::INT32_TAG),
    );
    let lo32 = ctx.block().trunc(I64, &bits, I32);
    let int32_as_double = ctx.block().sitofp(I32, &lo32, DOUBLE);
    let canonical = ctx
        .block()
        .select(I1, &is_int32, DOUBLE, &int32_as_double, dv);
    let is_untagged_number = emit_js_value_is_number(ctx, dv);
    let is_numeric = ctx.block().or(I1, &is_untagged_number, &is_int32);
    let root_idx = ctx.new_block("switch.bst");
    let root_label = ctx.block_label(root_idx);
    ctx.block()
        .cond_br(&is_numeric, &root_label, no_match_label);
    ctx.current_block = root_idx;
    emit_numeric_tree_range(
        ctx,
        &canonical,
        plan,
        0,
        plan.len() - 1,
        body_blocks,
        no_match_label,
    );
}

/// Fill `ctx.current_block` with the dispatch for `plan[lo..=hi]`:
///
/// ```text
///   fcmp oeq x, v[mid]  → body[mid]
///   fcmp olt x, v[mid]  → subtree(lo, mid-1)   (or no-match if empty)
///   otherwise           → subtree(mid+1, hi)   (or no-match if empty)
/// ```
///
/// Every leaf edge ends in an `oeq` guard, so a discriminant that is between
/// case values (e.g. 2.5 against integer cases), below the minimum, above the
/// maximum, or NaN (all ordered fcmps false → always takes the ≥ edge until
/// the run ends) lands on the no-match label.
fn emit_numeric_tree_range(
    ctx: &mut FnCtx<'_>,
    canonical: &str,
    plan: &[(f64, usize)],
    lo: usize,
    hi: usize,
    body_blocks: &[usize],
    no_match_label: &str,
) {
    let mid = lo + (hi - lo) / 2;
    let (value, case_idx) = plan[mid];
    let value_lit = crate::nanbox::double_literal(value);
    let body_label = ctx.block_label(body_blocks[case_idx]);

    let eq = ctx.block().fcmp("oeq", canonical, &value_lit);
    if lo == hi {
        ctx.block().cond_br(&eq, &body_label, no_match_label);
        return;
    }

    let cmp_idx = ctx.new_block("switch.bst");
    let cmp_label = ctx.block_label(cmp_idx);
    ctx.block().cond_br(&eq, &body_label, &cmp_label);
    ctx.current_block = cmp_idx;

    let lt = ctx.block().fcmp("olt", canonical, &value_lit);
    let left = (mid > lo).then(|| ctx.new_block("switch.bst"));
    let right = (mid < hi).then(|| ctx.new_block("switch.bst"));
    let left_label = left.map_or_else(|| no_match_label.to_string(), |b| ctx.block_label(b));
    let right_label = right.map_or_else(|| no_match_label.to_string(), |b| ctx.block_label(b));
    ctx.block().cond_br(&lt, &left_label, &right_label);

    if let Some(b) = left {
        ctx.current_block = b;
        emit_numeric_tree_range(
            ctx,
            canonical,
            plan,
            lo,
            mid - 1,
            body_blocks,
            no_match_label,
        );
    }
    if let Some(b) = right {
        ctx.current_block = b;
        emit_numeric_tree_range(
            ctx,
            canonical,
            plan,
            mid + 1,
            hi,
            body_blocks,
            no_match_label,
        );
    }
}
