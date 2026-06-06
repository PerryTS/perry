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

    // Branch from the discriminant block into the first test (or
    // straight into the body if there are zero cases — degenerate but
    // legal).
    if let Some(&first_test) = test_blocks.first() {
        let first_test_label = ctx.block_label(first_test);
        ctx.block().br(&first_test_label);
    } else {
        ctx.block().br(&exit_label);
        ctx.current_block = exit_idx;
        return Ok(());
    }

    // Find the default case index, if any. The "fall-through to default
    // when nothing matches" target is the default's body block; if
    // there's no default, we fall through to exit.
    let default_idx = cases.iter().position(|c| c.test.is_none());
    let no_match_target_label = match default_idx {
        Some(i) => ctx.block_label(body_blocks[i]),
        None => exit_label.clone(),
    };

    // Push break target. Switch has no continue, so we use exit for both.
    ctx.loop_targets
        .push((exit_label.clone(), exit_label.clone(), ctx.try_depth));

    // Compile each test block. Each test compares dv against the case
    // expression with fcmp oeq, jumps to the body on match, otherwise
    // jumps to the next test (or to no_match_target if this is the last).
    for (i, case) in cases.iter().enumerate() {
        ctx.current_block = test_blocks[i];
        let body_label = ctx.block_label(body_blocks[i]);
        let next_label = if i + 1 < test_blocks.len() {
            ctx.block_label(test_blocks[i + 1])
        } else {
            no_match_target_label.clone()
        };

        if let Some(test_expr) = case.test.as_ref() {
            let cv = lower_expr(ctx, test_expr)?;
            // If either the discriminant or the case value is a static
            // string expression (e.g. `switch (typeof x) { case "foo": }`),
            // compare by string content via js_string_equals. Two allocations
            // of the same text have different pointers, so icmp on bits
            // would report them unequal. Dispatch through the unified
            // string-pointer getter which returns null for non-strings —
            // js_string_equals treats null as "not equal", matching the
            // expected fall-through behavior.
            let either_string = crate::type_analysis::is_string_expr(ctx, discriminant)
                || crate::type_analysis::is_string_expr(ctx, test_expr);
            if either_string {
                let blk = ctx.block();
                let l_handle = blk.call(
                    crate::types::I64,
                    "js_get_string_pointer_unified",
                    &[(crate::types::DOUBLE, &dv)],
                );
                let r_handle = blk.call(
                    crate::types::I64,
                    "js_get_string_pointer_unified",
                    &[(crate::types::DOUBLE, &cv)],
                );
                let i32_eq = blk.call(
                    crate::types::I32,
                    "js_string_equals",
                    &[
                        (crate::types::I64, &l_handle),
                        (crate::types::I64, &r_handle),
                    ],
                );
                let cmp = blk.icmp_ne(crate::types::I32, &i32_eq, "0");
                blk.cond_br(&cmp, &body_label, &next_label);
            } else {
                // fcmp on NaN-tagged string/pointer values is always
                // false (NaN comparisons are unordered). For switch on
                // strings or any value that might be NaN-tagged, compare
                // the i64 bit patterns instead. This works for numbers
                // too — equal doubles have equal bits except for ±0
                // which the JS spec treats as equal anyway and Number(0)
                // === Number(-0) is true.
                let blk = ctx.block();
                let dv_bits = blk.bitcast_double_to_i64(&dv);
                let cv_bits = blk.bitcast_double_to_i64(&cv);
                let cmp = blk.icmp_eq(crate::types::I64, &dv_bits, &cv_bits);
                blk.cond_br(&cmp, &body_label, &next_label);
            }
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
