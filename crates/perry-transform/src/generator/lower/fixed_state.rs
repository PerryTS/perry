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
}
