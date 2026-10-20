//! Bindings visible to user closures keep their shared cells. The remaining
//! generator captures belong solely to its single resume record.

use super::*;
use std::collections::HashSet;

pub(super) fn collect_user_closure_refs(stmts: &[Stmt], out: &mut HashSet<LocalId>) {
    fn visit(expr: &Expr, out: &mut HashSet<LocalId>) {
        if let Expr::Closure {
            body,
            captures,
            mutable_captures,
            ..
        } = expr
        {
            out.extend(captures.iter().copied());
            out.extend(mutable_captures.iter().copied());
            let mut refs = Vec::new();
            let mut visited = HashSet::new();
            for stmt in body {
                perry_hir::analysis::collect_local_refs_stmt(stmt, &mut refs, &mut visited);
            }
            out.extend(refs);
        }
        perry_hir::walker::walk_expr_children(expr, &mut |child| visit(child, out));
    }
    for stmt in stmts {
        perry_hir::walker::stmt_any_expr(stmt, &mut |expr| {
            visit(expr, out);
            false
        });
    }
}

fn collect_box_prealloc_ids(stmts: &[Stmt], out: &mut Vec<LocalId>) {
    for stmt in stmts {
        match stmt {
            Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
                out.extend(ids.iter().copied())
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_box_prealloc_ids(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_box_prealloc_ids(eb, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_box_prealloc_ids(body, out)
            }
            Stmt::Labeled { body, .. } => {
                collect_box_prealloc_ids(std::slice::from_ref(&**body), out)
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    collect_box_prealloc_ids(std::slice::from_ref(&**init), out);
                }
                collect_box_prealloc_ids(body, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_box_prealloc_ids(body, out);
                if let Some(c) = catch {
                    collect_box_prealloc_ids(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_box_prealloc_ids(f, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_box_prealloc_ids(&case.body, out);
                }
            }
            _ => {}
        }
    }
}

/// A declaration and all its uses in one resume state need no persistent
/// slot. Keep shared bindings and abrupt-route operands conservative: their
/// lifetime can extend beyond the state's ordinary exit.
pub(super) fn state_local_ids(
    states: &[State],
    shared: &HashSet<LocalId>,
    catches: &[CatchRoute],
    finallys: &[FinallyRoute],
    delegations: &[DelegationRoute],
) -> HashSet<LocalId> {
    use std::collections::HashMap;
    let mut owners: HashMap<LocalId, Option<u32>> = HashMap::new();
    let mut declared = HashSet::new();
    let mut force_persistent = shared.clone();
    let refs = |stmts: &[Stmt]| {
        let mut ids = Vec::new();
        let mut visited = HashSet::new();
        for stmt in stmts {
            perry_hir::analysis::collect_local_refs_stmt(stmt, &mut ids, &mut visited);
        }
        ids
    };
    for route in catches {
        force_persistent.extend(refs(&route.body));
        force_persistent.extend(route.param_id);
    }
    for route in finallys {
        force_persistent.extend(refs(&route.body));
    }
    for route in delegations {
        force_persistent.extend([route.iter_id, route.result_id]);
    }
    for state in states {
        let declarations: Vec<_> = collect_hoisted_vars(&state.body)
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        declared.extend(declarations.iter().copied());
        let mut uses = refs(&state.body);
        uses.extend(declarations);
        // A box preallocated in this state belongs to the binding's lifetime:
        // the declaring state writes through that box. HIR preallocates a
        // forward-captured `let`/`const` (name-based, so possibly
        // over-collected) at its block entry, which can be an earlier state.
        collect_box_prealloc_ids(&state.body, &mut uses);
        if let StateExit::Yield { value, .. } | StateExit::Await { value, .. } = &state.exit {
            perry_hir::analysis::collect_local_refs_expr(value, &mut uses, &mut HashSet::new());
        }
        for id in uses {
            owners
                .entry(id)
                .and_modify(|owner| {
                    if *owner != Some(state.num) {
                        *owner = None;
                    }
                })
                .or_insert(Some(state.num));
        }
    }
    declared
        .retain(|id| !force_persistent.contains(id) && owners.get(id).is_some_and(Option::is_some));
    declared
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_one_state_locals_do_not_displace_shared_or_cross_state_storage() {
        let declare = |id| Stmt::Let {
            id,
            name: format!("local{id}"),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(7.0)),
        };
        let states = vec![
            State {
                num: 0,
                body: vec![declare(1), declare(2), declare(3)],
                exit: StateExit::Yield {
                    value: Expr::LocalGet(1),
                    next_state: 1,
                },
            },
            State {
                num: 1,
                body: vec![Stmt::Return(Some(Expr::LocalGet(2)))],
                exit: StateExit::Done,
            },
        ];
        assert_eq!(
            state_local_ids(&states, &HashSet::from([3]), &[], &[], &[]),
            HashSet::from([1])
        );
    }

    #[test]
    fn generator_box_preallocated_in_an_earlier_state_is_persistent() {
        let states = vec![
            State {
                num: 0,
                body: vec![Stmt::PreallocateTdzBoxes(vec![1])],
                exit: StateExit::Yield {
                    value: Expr::Number(1.0),
                    next_state: 1,
                },
            },
            State {
                num: 1,
                body: vec![
                    Stmt::Let {
                        id: 1,
                        name: "client".into(),
                        ty: Type::Any,
                        mutable: false,
                        init: Some(Expr::Number(7.0)),
                    },
                    Stmt::Return(Some(Expr::LocalGet(1))),
                ],
                exit: StateExit::Done,
            },
        ];
        assert!(state_local_ids(&states, &HashSet::new(), &[], &[], &[]).is_empty());
    }
}
