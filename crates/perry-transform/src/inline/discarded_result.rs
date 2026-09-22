//! Inline boundary for a call whose result is discarded (`f(x);`).
//!
//! A statement-position inline splices the callee body into the caller's
//! statement list, so any `return` left in that body returns from the CALLER
//! (#10416: decimal.js `intPow` returned the `true` of its `truncate(…);`
//! helper, and at module top level the stray `ret double` did not even match
//! `main`'s `i32` result type). The value arm (`let r = f(x)`) solves the same
//! problem with a `do { … } while (false)` wrapper whose converted returns
//! `break` out after writing `r`. A discarded result has no variable to
//! write, so the returns are removed structurally instead, without adding a
//! loop to the caller (a loop brings a GC back-edge poll with it and changes
//! the shape of any hot loop the call sits in):
//!
//! * `return e;` becomes `e;` (the value is dropped, its evaluation is not),
//!   and `return;` becomes nothing;
//! * the statements after an `if` that returns move into the branch that can
//!   still fall through to them: `if (c) { a; return v; } rest` becomes
//!   `if (c) { a; v; } else { rest }`.
//!
//! The rewrite adds no loop or label, so a `break`/`continue` in the callee
//! keeps its target. It declines (`None`: keep the call) rather than
//! duplicate or drop statements: when both branches of such an `if` can fall
//! through to a non-empty continuation, when statements follow a `return` or
//! an `if` whose branches all exit, and when a `return` sits under anything
//! other than `if` nesting (loop, `switch`, `try`, label).
//! `has_simple_control_flow` keeps most of those shapes out of inline
//! candidates today; the boundary does not rely on it.

use perry_hir::Stmt;

use super::call_inliner::stmt_contains_return;

/// Remove every `return` from an inlined callee body whose result is
/// discarded, or `None` when that cannot be done without duplicating or
/// dropping statements (see the module docs).
pub(crate) fn discard_inlined_returns(stmts: Vec<Stmt>) -> Option<Vec<Stmt>> {
    let mut out = Vec::with_capacity(stmts.len());
    let mut stmts = stmts.into_iter();
    while let Some(stmt) = stmts.next() {
        match stmt {
            Stmt::Return(value) => {
                if !stmts.as_slice().is_empty() {
                    return None;
                }
                out.extend(value.map(Stmt::Expr));
                return Some(out);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } if then_branch.iter().any(stmt_contains_return)
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| branch.iter().any(stmt_contains_return)) =>
            {
                let continuation: Vec<Stmt> = stmts.collect();
                let mut then_branch = then_branch;
                let mut else_branch = else_branch.unwrap_or_default();
                if !continuation.is_empty() {
                    match (
                        can_complete_normally(&then_branch),
                        can_complete_normally(&else_branch),
                    ) {
                        (true, false) => then_branch.extend(continuation),
                        (false, true) => else_branch.extend(continuation),
                        // Both fall through: the continuation would have to be
                        // duplicated. Neither does: it is unreachable, and
                        // dropping it is not this rewrite's decision.
                        (true, true) | (false, false) => return None,
                    }
                }
                let then_branch = discard_inlined_returns(then_branch)?;
                let else_branch = discard_inlined_returns(else_branch)?;
                out.push(Stmt::If {
                    condition,
                    then_branch,
                    else_branch: (!else_branch.is_empty()).then_some(else_branch),
                });
                return Some(out);
            }
            other if stmt_contains_return(&other) => return None,
            other => out.push(other),
        }
    }
    Some(out)
}

/// `false` only when every path through `stmts` ends in `return` or `throw`.
/// Answering `true` for a list that cannot fall through is safe (the
/// continuation lands after an exit and the rewrite then declines, or it is
/// dead code after a `throw`); answering `false` for one that can would skip
/// the continuation on that path.
fn can_complete_normally(stmts: &[Stmt]) -> bool {
    !stmts.iter().any(|stmt| match stmt {
        Stmt::Return(_) | Stmt::Throw(_) => true,
        Stmt::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => !can_complete_normally(then_branch) && !can_complete_normally(else_branch),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::Expr;

    fn ret(value: i64) -> Stmt {
        Stmt::Return(Some(Expr::Integer(value)))
    }

    fn effect(value: i64) -> Stmt {
        Stmt::Expr(Expr::Integer(value))
    }

    fn cond(value: i64) -> Expr {
        Expr::LocalGet(value as u32)
    }

    fn if_stmt(condition: i64, then_branch: Vec<Stmt>, else_branch: Option<Vec<Stmt>>) -> Stmt {
        Stmt::If {
            condition: cond(condition),
            then_branch,
            else_branch,
        }
    }

    fn contains_return(stmts: &[Stmt]) -> bool {
        stmts.iter().any(stmt_contains_return)
    }

    fn dump(stmts: &[Stmt]) -> String {
        format!("{stmts:?}")
    }

    #[test]
    fn trailing_bare_return_becomes_its_value() {
        let out = discard_inlined_returns(vec![effect(1), ret(2)]).unwrap();
        assert_eq!(dump(&out), dump(&[effect(1), effect(2)]));
        let out = discard_inlined_returns(vec![effect(1), Stmt::Return(None)]).unwrap();
        assert_eq!(dump(&out), dump(&[effect(1)]));
    }

    /// #10416's shape: the callee's LAST statement is an `if` that returns.
    /// The pre-fix statement arm only inspected `take(len - 1)` and only
    /// rewrote a bare trailing `Return`, so this `return` reached the caller.
    #[test]
    fn trailing_if_return_loses_its_return() {
        let body = vec![effect(1), if_stmt(7, vec![effect(2), ret(3)], None)];
        let out = discard_inlined_returns(body).unwrap();
        assert!(!contains_return(&out), "{}", dump(&out));
        assert_eq!(
            dump(&out),
            dump(&[effect(1), if_stmt(7, vec![effect(2), effect(3)], None)])
        );
    }

    #[test]
    fn guard_moves_the_continuation_into_the_falling_branch() {
        // if (c) { return 1; } a; return 2;   =>   if (c) { 1; } else { a; 2; }
        let body = vec![if_stmt(7, vec![ret(1)], None), effect(5), ret(2)];
        let out = discard_inlined_returns(body).unwrap();
        assert_eq!(
            dump(&out),
            dump(&[if_stmt(
                7,
                vec![effect(1)],
                Some(vec![effect(5), effect(2)])
            )])
        );

        // if (c) { a; } else { return 1; } b;   =>   if (c) { a; b; } else { 1; }
        let body = vec![if_stmt(7, vec![effect(4)], Some(vec![ret(1)])), effect(5)];
        let out = discard_inlined_returns(body).unwrap();
        assert_eq!(
            dump(&out),
            dump(&[if_stmt(
                7,
                vec![effect(4), effect(5)],
                Some(vec![effect(1)])
            )])
        );
    }

    #[test]
    fn both_branches_returning_and_nested_guards() {
        let out =
            discard_inlined_returns(vec![if_stmt(7, vec![ret(1)], Some(vec![ret(2)]))]).unwrap();
        assert_eq!(
            dump(&out),
            dump(&[if_stmt(7, vec![effect(1)], Some(vec![effect(2)]))])
        );

        // if (a) { if (b) return 1; x; } else { throw } y;
        let body = vec![
            if_stmt(
                7,
                vec![if_stmt(8, vec![ret(1)], None), effect(3)],
                Some(vec![Stmt::Throw(Expr::Integer(9))]),
            ),
            effect(4),
        ];
        let out = discard_inlined_returns(body).unwrap();
        assert!(!contains_return(&out), "{}", dump(&out));
        assert_eq!(
            dump(&out),
            dump(&[if_stmt(
                7,
                vec![if_stmt(
                    8,
                    vec![effect(1)],
                    Some(vec![effect(3), effect(4)])
                )],
                Some(vec![Stmt::Throw(Expr::Integer(9))]),
            )])
        );
    }

    #[test]
    fn declines_instead_of_duplicating_or_dropping_statements() {
        // Both branches can fall through to `rest`.
        let body = vec![
            if_stmt(7, vec![if_stmt(8, vec![ret(1)], None), effect(2)], None),
            effect(3),
        ];
        assert!(discard_inlined_returns(body).is_none());

        // Statements after an unconditional return / an if whose branches all exit.
        assert!(discard_inlined_returns(vec![ret(1), effect(2)]).is_none());
        let body = vec![if_stmt(7, vec![ret(1)], Some(vec![ret(2)])), effect(3)];
        assert!(discard_inlined_returns(body).is_none());
    }

    #[test]
    fn declines_returns_outside_if_nesting() {
        let in_loop = Stmt::While {
            condition: cond(7),
            body: vec![if_stmt(8, vec![ret(1)], None), Stmt::Break],
        };
        assert!(discard_inlined_returns(vec![in_loop]).is_none());
        let labeled = Stmt::Labeled {
            label: "l".to_string(),
            body: Box::new(if_stmt(8, vec![ret(1)], None)),
        };
        assert!(discard_inlined_returns(vec![effect(1), labeled]).is_none());
        let nested = if_stmt(
            7,
            vec![Stmt::DoWhile {
                body: vec![ret(1)],
                condition: Expr::Bool(false),
            }],
            None,
        );
        assert!(discard_inlined_returns(vec![nested]).is_none());
    }

    #[test]
    fn return_free_bodies_are_untouched() {
        let body = vec![
            effect(1),
            Stmt::While {
                condition: cond(7),
                body: vec![if_stmt(8, vec![Stmt::Break], None), Stmt::Continue],
            },
            if_stmt(9, vec![Stmt::Throw(Expr::Integer(2))], None),
        ];
        let expected = dump(&body);
        assert_eq!(dump(&discard_inlined_returns(body).unwrap()), expected);
    }

    // ---- through the inliner ------------------------------------------------

    use crate::inline::call_inliner::try_inline_simple_call;
    use crate::inline::{inline_functions, ExactReceiverFact, ExactReceiverFacts, MethodCandidate};
    use perry_hir::types::{FuncId, LocalId, Type};
    use perry_hir::{CompareOp, Function, Module, Param};
    use std::collections::HashMap;

    fn function(id: FuncId, params: Vec<LocalId>, body: Vec<Stmt>) -> Function {
        Function {
            id,
            name: format!("f{id}"),
            type_params: Vec::new(),
            params: params
                .into_iter()
                .map(|id| Param {
                    id,
                    name: format!("p{id}"),
                    ty: Type::Number,
                    default: None,
                    decorators: Vec::new(),
                    is_rest: false,
                    arguments_object: None,
                })
                .collect(),
            return_type: Type::Any,
            body,
            is_async: false,
            is_generator: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
            is_strict: false,
        }
    }

    fn call(func: FuncId, args: Vec<Expr>) -> Expr {
        Expr::Call {
            callee: Box::new(Expr::FuncRef(func)),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
        }
    }

    fn greater(local: LocalId, value: i64) -> Expr {
        Expr::Compare {
            op: CompareOp::Gt,
            left: Box::new(Expr::LocalGet(local)),
            right: Box::new(Expr::Integer(value)),
        }
    }

    fn inline(module: &mut Module) {
        inline_functions(
            module,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
    }

    /// The #10416 repro, end to end: `early(5);` in a function that is not
    /// itself a candidate (it has a loop) and in module init.
    #[test]
    fn statement_call_to_trailing_if_return_helper_no_longer_returns_from_caller() {
        let early = function(
            1,
            vec![1],
            vec![if_stmt_on(
                greater(1, 1),
                vec![Stmt::Return(Some(Expr::String("early".into())))],
            )],
        );
        let caller = function(
            2,
            Vec::new(),
            vec![
                Stmt::Expr(call(1, vec![Expr::Integer(5)])),
                Stmt::While {
                    condition: Expr::Bool(false),
                    body: Vec::new(),
                },
                Stmt::Return(Some(Expr::String("after".into()))),
            ],
        );
        let mut module = Module::new("inline-10416.ts");
        module.functions = vec![early, caller];
        module.init = vec![Stmt::Expr(call(1, vec![Expr::Integer(5)]))];

        inline(&mut module);

        let body = &module.functions[1].body;
        assert!(!format!("{body:?}").contains("FuncRef(1)"), "{body:?}");
        assert_eq!(body.len(), 3, "{body:?}");
        assert!(!stmt_contains_return(&body[0]), "{body:?}");
        assert!(matches!(&body[2], Stmt::Return(Some(Expr::String(s))) if s == "after"));
        let init = &module.init;
        assert!(!format!("{init:?}").contains("FuncRef(1)"), "{init:?}");
        assert!(!contains_return(init), "{init:?}");
    }

    /// When the discarded-result rewrite declines, the call statement must
    /// stay. The pre-fix fallback replaced it with the setup statements
    /// hoisted out of its arguments, silently deleting the call.
    #[test]
    fn declined_statement_call_is_kept_after_its_hoisted_argument_setup() {
        // f(x) { if (x > 0) { if (x > 5) return 1; 2; } 3; }
        let f = function(
            1,
            vec![1],
            vec![
                if_stmt_on(
                    greater(1, 0),
                    vec![if_stmt_on(greater(1, 5), vec![ret(1)]), effect(2)],
                ),
                effect(3),
            ],
        );
        // g(y) { const z = y + 0; return z; }
        let g = function(
            2,
            vec![2],
            vec![
                Stmt::Let {
                    id: 3,
                    name: "z".into(),
                    ty: Type::Number,
                    mutable: false,
                    init: Some(Expr::LocalGet(2)),
                },
                Stmt::Return(Some(Expr::LocalGet(3))),
            ],
        );
        let caller = function(
            3,
            vec![10],
            vec![
                Stmt::Expr(call(1, vec![call(2, vec![Expr::LocalGet(10)])])),
                Stmt::While {
                    condition: Expr::Bool(false),
                    body: Vec::new(),
                },
            ],
        );
        let mut module = Module::new("inline-10416-declined.ts");
        module.functions = vec![f, g, caller];

        inline(&mut module);

        let body = &module.functions[2].body;
        assert!(!format!("{body:?}").contains("FuncRef(2)"), "{body:?}");
        let call_at = body
            .iter()
            .position(|stmt| matches!(stmt, Stmt::Expr(Expr::Call { callee, .. }) if matches!(callee.as_ref(), Expr::FuncRef(1))))
            .unwrap_or_else(|| panic!("the call to f must survive: {body:?}"));
        assert!(
            matches!(&body[..call_at], [Stmt::Let { .. }]),
            "g's setup must precede the kept call: {body:?}"
        );
    }

    /// A void method whose `return;` precedes more statements must not have
    /// those dead statements spliced into an expression-position call site.
    #[test]
    fn void_method_statements_after_return_are_not_spliced() {
        let set_x = Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: "x".into(),
            value: Box::new(Expr::Integer(1)),
        });
        let candidate = |body: Vec<Stmt>| {
            let mut methods = HashMap::new();
            methods.insert(
                ("Dead".to_string(), "m".to_string()),
                MethodCandidate {
                    func: function(1, Vec::new(), body),
                    this_param_id: None,
                    method_lookup_safe: true,
                    required_extern_imports: Vec::new(),
                },
            );
            methods
        };
        let mut facts = ExactReceiverFacts::new();
        facts.insert(
            7,
            ExactReceiverFact {
                class_name: "Dead".into(),
            },
        );
        let method_call = Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(7)),
                property: "m".into(),
                byte_offset: 0,
            }),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        };
        let try_inline = |methods: &HashMap<(String, String), MethodCandidate>| {
            let mut next_local_id = 100;
            try_inline_simple_call(
                &method_call,
                &HashMap::new(),
                methods,
                &HashMap::new(),
                &facts,
                &mut next_local_id,
                None,
                &HashMap::new(),
            )
        };

        let dead = candidate(vec![Stmt::Return(None), set_x.clone()]);
        assert!(try_inline(&dead).is_none());
        let live = candidate(vec![set_x, Stmt::Return(None)]);
        let (stmts, result) = try_inline(&live).expect("a trailing `return;` still inlines");
        assert_eq!(stmts.len(), 1);
        assert!(matches!(result, Expr::Undefined));
    }

    fn if_stmt_on(condition: Expr, then_branch: Vec<Stmt>) -> Stmt {
        Stmt::If {
            condition,
            then_branch,
            else_branch: None,
        }
    }
}
