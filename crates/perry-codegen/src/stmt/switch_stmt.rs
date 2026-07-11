//! `Stmt::Switch` lowering.

use super::*;

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
pub(crate) fn lower_switch(
    ctx: &mut FnCtx<'_>,
    discriminant: &perry_hir::Expr,
    cases: &[perry_hir::SwitchCase],
) -> Result<()> {
    let dv = lower_expr(ctx, discriminant)?;

    // Allocate test/body blocks for every case up front so we can wire
    // up the fall-through edges before each block is filled in.
    let mut test_blocks: Vec<usize> = Vec::with_capacity(cases.len());
    let mut body_blocks: Vec<usize> = Vec::with_capacity(cases.len());
    for (i, case) in cases.iter().enumerate() {
        let test_name = if case.test.is_some() {
            format!("switch.test{}", i)
        } else {
            format!("switch.default_test{}", i)
        };
        test_blocks.push(ctx.new_block(&test_name));
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

    // Branch from the discriminant block into the first *case* test.
    // A leading `default:` is skipped — its body only runs when no case
    // test anywhere in the block matches.
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

    // Compile each test block. Each test compares dv against the case
    // expression with fcmp oeq, jumps to the body on match, otherwise
    // jumps to the next *case* test (or to no_match_target if this is the
    // last). The default clause is NOT part of the test chain: per spec
    // CaseBlockEvaluation, every case test — including ones written
    // *after* `default:` — is tried first, and the default body only
    // runs when no case matched. A non-match at the default's source
    // position must therefore skip over it to the next case test.
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
