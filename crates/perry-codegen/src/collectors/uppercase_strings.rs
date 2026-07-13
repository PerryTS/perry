//! Conservative use analysis for non-escaping `toUpperCase()` results.
//!
//! The code generator can keep such a result virtual only when every read is
//! handled by a fused runtime operation. This pass deliberately admits just
//! `indexOf("literal")` and a literal-separator split whose parts are read
//! only through constant-index `.length`.

use std::collections::{HashMap, HashSet};

use perry_hir::{Expr, Stmt};

pub(crate) fn collect_fusible_uppercase_locals(
    stmts: &[Stmt],
    non_escaping_arrays: &HashMap<u32, u32>,
    used_array_indices: &HashMap<u32, HashSet<u32>>,
    length_only_array_indices: &HashMap<u32, HashSet<u32>>,
) -> HashSet<u32> {
    let mut candidates = HashSet::new();
    find_candidates(stmts, &mut candidates);
    if candidates.is_empty() {
        return candidates;
    }

    let mut valid = candidates.clone();
    check_uses_in_stmts(
        stmts,
        &candidates,
        &mut valid,
        non_escaping_arrays,
        used_array_indices,
        length_only_array_indices,
    );
    valid
}

fn find_candidates(stmts: &[Stmt], candidates: &mut HashSet<u32>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                id,
                init: Some(Expr::Call { callee, args, .. }),
                ..
            } if args.is_empty()
                && matches!(
                    callee.as_ref(),
                    Expr::PropertyGet { object, property }
                        if matches!(object.as_ref(), Expr::LocalGet(_))
                            && property == "toUpperCase"
                ) =>
            {
                candidates.insert(*id);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                find_candidates(then_branch, candidates);
                if let Some(else_branch) = else_branch {
                    find_candidates(else_branch, candidates);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                find_candidates(body, candidates);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    find_candidates(std::slice::from_ref(init.as_ref()), candidates);
                }
                find_candidates(body, candidates);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                find_candidates(body, candidates);
                if let Some(catch) = catch {
                    find_candidates(&catch.body, candidates);
                }
                if let Some(finally) = finally {
                    find_candidates(finally, candidates);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    find_candidates(&case.body, candidates);
                }
            }
            Stmt::Labeled { body, .. } => {
                find_candidates(std::slice::from_ref(body.as_ref()), candidates);
            }
            _ => {}
        }
    }
}

fn split_parts_are_length_only(
    id: u32,
    non_escaping_arrays: &HashMap<u32, u32>,
    used_array_indices: &HashMap<u32, HashSet<u32>>,
    length_only_array_indices: &HashMap<u32, HashSet<u32>>,
) -> bool {
    non_escaping_arrays.contains_key(&id)
        && used_array_indices.get(&id).is_some_and(|used| {
            !used.is_empty() && length_only_array_indices.get(&id) == Some(used)
        })
}

fn check_uses_in_stmts(
    stmts: &[Stmt],
    candidates: &HashSet<u32>,
    valid: &mut HashSet<u32>,
    non_escaping_arrays: &HashMap<u32, u32>,
    used_array_indices: &HashMap<u32, HashSet<u32>>,
    length_only_array_indices: &HashMap<u32, HashSet<u32>>,
) {
    for stmt in stmts {
        if let Stmt::Let {
            id,
            init: Some(Expr::Call { callee, args, .. }),
            ..
        } = stmt
        {
            if args.len() == 1
                && matches!(args.as_slice(), [Expr::String(separator)] if !separator.is_empty())
                && matches!(
                    callee.as_ref(),
                    Expr::PropertyGet { object, property }
                        if matches!(object.as_ref(), Expr::LocalGet(_)) && property == "split"
                )
                && split_parts_are_length_only(
                    *id,
                    non_escaping_arrays,
                    used_array_indices,
                    length_only_array_indices,
                )
            {
                let Expr::PropertyGet { object, .. } = callee.as_ref() else {
                    unreachable!();
                };
                let Expr::LocalGet(upper_id) = object.as_ref() else {
                    unreachable!();
                };
                if candidates.contains(upper_id) {
                    // This is an admitted use. Do not recurse into the
                    // receiver, which would otherwise look like a raw escape.
                    continue;
                }
            }
        }

        check_uses_in_stmt(
            stmt,
            candidates,
            valid,
            non_escaping_arrays,
            used_array_indices,
            length_only_array_indices,
        );
    }
}

fn check_uses_in_stmt(
    stmt: &Stmt,
    candidates: &HashSet<u32>,
    valid: &mut HashSet<u32>,
    non_escaping_arrays: &HashMap<u32, u32>,
    used_array_indices: &HashMap<u32, HashSet<u32>>,
    length_only_array_indices: &HashMap<u32, HashSet<u32>>,
) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(init) = init {
                check_uses_in_expr(
                    init,
                    candidates,
                    valid,
                    non_escaping_arrays,
                    used_array_indices,
                    length_only_array_indices,
                );
            }
        }
        Stmt::Expr(expr) | Stmt::Throw(expr) => check_uses_in_expr(
            expr,
            candidates,
            valid,
            non_escaping_arrays,
            used_array_indices,
            length_only_array_indices,
        ),
        Stmt::Return(expr) => {
            if let Some(expr) = expr {
                check_uses_in_expr(
                    expr,
                    candidates,
                    valid,
                    non_escaping_arrays,
                    used_array_indices,
                    length_only_array_indices,
                );
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            check_uses_in_expr(
                condition,
                candidates,
                valid,
                non_escaping_arrays,
                used_array_indices,
                length_only_array_indices,
            );
            check_uses_in_stmts(
                then_branch,
                candidates,
                valid,
                non_escaping_arrays,
                used_array_indices,
                length_only_array_indices,
            );
            if let Some(else_branch) = else_branch {
                check_uses_in_stmts(
                    else_branch,
                    candidates,
                    valid,
                    non_escaping_arrays,
                    used_array_indices,
                    length_only_array_indices,
                );
            }
        }
        Stmt::While { condition, body } => {
            check_uses_in_expr(
                condition,
                candidates,
                valid,
                non_escaping_arrays,
                used_array_indices,
                length_only_array_indices,
            );
            check_uses_in_stmts(
                body,
                candidates,
                valid,
                non_escaping_arrays,
                used_array_indices,
                length_only_array_indices,
            );
        }
        Stmt::DoWhile { body, condition } => {
            check_uses_in_stmts(
                body,
                candidates,
                valid,
                non_escaping_arrays,
                used_array_indices,
                length_only_array_indices,
            );
            check_uses_in_expr(
                condition,
                candidates,
                valid,
                non_escaping_arrays,
                used_array_indices,
                length_only_array_indices,
            );
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                check_uses_in_stmts(
                    std::slice::from_ref(init.as_ref()),
                    candidates,
                    valid,
                    non_escaping_arrays,
                    used_array_indices,
                    length_only_array_indices,
                );
            }
            if let Some(condition) = condition {
                check_uses_in_expr(
                    condition,
                    candidates,
                    valid,
                    non_escaping_arrays,
                    used_array_indices,
                    length_only_array_indices,
                );
            }
            if let Some(update) = update {
                check_uses_in_expr(
                    update,
                    candidates,
                    valid,
                    non_escaping_arrays,
                    used_array_indices,
                    length_only_array_indices,
                );
            }
            check_uses_in_stmts(
                body,
                candidates,
                valid,
                non_escaping_arrays,
                used_array_indices,
                length_only_array_indices,
            );
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            check_uses_in_stmts(
                body,
                candidates,
                valid,
                non_escaping_arrays,
                used_array_indices,
                length_only_array_indices,
            );
            if let Some(catch) = catch {
                check_uses_in_stmts(
                    &catch.body,
                    candidates,
                    valid,
                    non_escaping_arrays,
                    used_array_indices,
                    length_only_array_indices,
                );
            }
            if let Some(finally) = finally {
                check_uses_in_stmts(
                    finally,
                    candidates,
                    valid,
                    non_escaping_arrays,
                    used_array_indices,
                    length_only_array_indices,
                );
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            check_uses_in_expr(
                discriminant,
                candidates,
                valid,
                non_escaping_arrays,
                used_array_indices,
                length_only_array_indices,
            );
            for case in cases {
                if let Some(test) = &case.test {
                    check_uses_in_expr(
                        test,
                        candidates,
                        valid,
                        non_escaping_arrays,
                        used_array_indices,
                        length_only_array_indices,
                    );
                }
                check_uses_in_stmts(
                    &case.body,
                    candidates,
                    valid,
                    non_escaping_arrays,
                    used_array_indices,
                    length_only_array_indices,
                );
            }
        }
        Stmt::Labeled { body, .. } => check_uses_in_stmts(
            std::slice::from_ref(body.as_ref()),
            candidates,
            valid,
            non_escaping_arrays,
            used_array_indices,
            length_only_array_indices,
        ),
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

fn check_uses_in_expr(
    expr: &Expr,
    candidates: &HashSet<u32>,
    valid: &mut HashSet<u32>,
    non_escaping_arrays: &HashMap<u32, u32>,
    used_array_indices: &HashMap<u32, HashSet<u32>>,
    length_only_array_indices: &HashMap<u32, HashSet<u32>>,
) {
    if let Expr::Call { callee, args, .. } = expr {
        if args.len() == 1
            && matches!(args.as_slice(), [Expr::String(_)])
            && matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property }
                    if matches!(object.as_ref(), Expr::LocalGet(_)) && property == "indexOf"
            )
        {
            let Expr::PropertyGet { object, .. } = callee.as_ref() else {
                unreachable!()
            };
            let Expr::LocalGet(id) = object.as_ref() else {
                unreachable!()
            };
            if candidates.contains(id) {
                return;
            }
        }
    }
    if let Expr::LocalGet(id) = expr {
        if candidates.contains(id) {
            valid.remove(id);
        }
        return;
    }
    if let Expr::Closure { body, captures, .. } = expr {
        for capture in captures {
            if candidates.contains(capture) {
                valid.remove(capture);
            }
        }
        check_uses_in_stmts(
            body,
            candidates,
            valid,
            non_escaping_arrays,
            used_array_indices,
            length_only_array_indices,
        );
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        check_uses_in_expr(
            child,
            candidates,
            valid,
            non_escaping_arrays,
            used_array_indices,
            length_only_array_indices,
        );
    });
}
