use super::*;

/// Rewrite completions targeting a source label in one branch of the
/// `ArrayIterationPatched` for-of guard. Each branch consists of setup
/// statements followed by the generated loop; nested loops in the user's body
/// remain boundaries inside [`rewrite_labeled_bc_in_stmts`].
pub(super) fn rewrite_labeled_bc_in_lowered_for_of_arm(
    stmts: &mut [Stmt],
    label: &str,
    next_local_id: &mut u32,
) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::For { body, .. } | Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                super::super::break_continue::desugar_labeled_escape_across_nested_loops(
                    body,
                    label,
                    next_local_id,
                );
                rewrite_labeled_bc_in_stmts(body, label, next_local_id);
            }
            _ => {}
        }
    }
}

/// Within a labeled loop's body, rewrite `break label` / `continue label`
/// that target THIS label into plain `break` / `continue`, so the loop's own
/// For/While linearization (which only knows about plain break/continue) maps
/// them to the loop's state targets. Descends through `if` / `try`, which do
/// not capture either completion. Nested loops remain a boundary because a
/// plain break/continue there would bind to the nested loop.
///
/// A switch is also a boundary for a plain `break`, but not for a labeled
/// break targeting the enclosing loop. When such a break is present, desugar
/// the switch first: its own plain breaks become the switch done-flag, while
/// the still-named outer break lands in the resulting `if` chain and can then
/// safely become the enclosing loop's plain break. This is especially
/// important for source `label: switch (...)` statements: HIR represents the
/// non-loop label as a labeled run-once do-while, and generator linearization
/// otherwise drops that label target while splitting an awaited case (#9186).
pub(super) fn rewrite_labeled_bc_in_stmts(
    stmts: &mut Vec<Stmt>,
    label: &str,
    next_local_id: &mut u32,
) {
    let mut i = 0;
    while i < stmts.len() {
        let desugared_switch = match &stmts[i] {
            Stmt::Switch {
                discriminant,
                cases,
            } if cases
                .iter()
                .any(|case| stmts_have_labeled_break_for(&case.body, label)) =>
            {
                Some(super::super::break_continue::desugar_switch_to_ifs(
                    discriminant,
                    cases,
                    next_local_id,
                ))
            }
            _ => None,
        };
        if let Some(desugared) = desugared_switch {
            stmts.splice(i..=i, desugared);
            // Reprocess at the same position. The replacement consists of
            // lets/ifs, so recursion below rewrites the preserved named break.
            continue;
        }

        let s = &mut stmts[i];
        match s {
            Stmt::LabeledBreak(l) if l == label => *s = Stmt::Break,
            Stmt::LabeledContinue(l) if l == label => *s = Stmt::Continue,
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                rewrite_labeled_bc_in_stmts(then_branch, label, next_local_id);
                if let Some(eb) = else_branch.as_mut() {
                    rewrite_labeled_bc_in_stmts(eb, label, next_local_id);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                rewrite_labeled_bc_in_stmts(body, label, next_local_id);
                if let Some(c) = catch.as_mut() {
                    rewrite_labeled_bc_in_stmts(&mut c.body, label, next_local_id);
                }
                if let Some(f) = finally.as_mut() {
                    rewrite_labeled_bc_in_stmts(f, label, next_local_id);
                }
            }
            // #5975: a `continue <label>` that targets THIS enclosing labeled
            // loop from inside a nested `switch` case. A switch never captures
            // `continue`, so it continues the loop — rewrite it to a plain
            // `continue` here so the loop's linearization (and the #5868
            // yielding-switch desugar) map it to the loop's re-entry sentinel.
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    rewrite_labeled_continue_in_stmts(&mut case.body, label);
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// Whether a statement list can reach `break label` without crossing a loop
/// or another labeled statement. Switches do not capture named breaks, so they
/// are traversed and individually desugared by the caller when necessary.
pub(super) fn stmts_have_labeled_break_for(stmts: &[Stmt], label: &str) -> bool {
    stmts.iter().any(|stmt| match stmt {
        Stmt::LabeledBreak(found) => found == label,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmts_have_labeled_break_for(then_branch, label)
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| stmts_have_labeled_break_for(branch, label))
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            stmts_have_labeled_break_for(body, label)
                || catch
                    .as_ref()
                    .is_some_and(|clause| stmts_have_labeled_break_for(&clause.body, label))
                || finally
                    .as_ref()
                    .is_some_and(|body| stmts_have_labeled_break_for(body, label))
        }
        Stmt::Switch { cases, .. } => cases
            .iter()
            .any(|case| stmts_have_labeled_break_for(&case.body, label)),
        Stmt::For { .. } | Stmt::While { .. } | Stmt::DoWhile { .. } | Stmt::Labeled { .. } => {
            false
        }
        _ => false,
    })
}

/// Rewrite `continue <label>` → plain `continue` for `label`, descending
/// through `if` / `try` / `switch` (none of which capture `continue`) but
/// stopping at nested loops (which bind their own `continue`). Unlike
/// [`rewrite_labeled_bc_in_stmts`] this does NOT touch `break <label>`: it is
/// used only when recursing into a nested `switch`, where a plain `break`
/// would be captured by the switch rather than escaping to the loop (#5975).
pub(super) fn rewrite_labeled_continue_in_stmts(stmts: &mut [Stmt], label: &str) {
    for s in stmts.iter_mut() {
        match s {
            Stmt::LabeledContinue(l) if l == label => *s = Stmt::Continue,
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                rewrite_labeled_continue_in_stmts(then_branch, label);
                if let Some(eb) = else_branch.as_mut() {
                    rewrite_labeled_continue_in_stmts(eb, label);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                rewrite_labeled_continue_in_stmts(body, label);
                if let Some(c) = catch.as_mut() {
                    rewrite_labeled_continue_in_stmts(&mut c.body, label);
                }
                if let Some(f) = finally.as_mut() {
                    rewrite_labeled_continue_in_stmts(f, label);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    rewrite_labeled_continue_in_stmts(&mut case.body, label);
                }
            }
            _ => {}
        }
    }
}
