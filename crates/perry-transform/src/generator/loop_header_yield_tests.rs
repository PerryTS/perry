//! #10419: a `yield` in a loop HEADER (while / do-while condition, for
//! condition / update / init) must be split into resume states at any nesting
//! depth, not only when the loop is a direct statement of the generator body.

use super::*;

fn yield_num(v: f64) -> Expr {
    Expr::Yield {
        value: Some(Box::new(Expr::Number(v))),
        delegate: false,
    }
}

/// `((yield v), <global>)` — the comma form minifiers emit in loop tests.
fn comma_yield(v: f64) -> Expr {
    Expr::Sequence(vec![yield_num(v), Expr::GlobalGet(0)])
}

fn generator(body: Vec<Stmt>, is_async: bool) -> Function {
    Function {
        id: 1,
        name: "header_yield".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body,
        is_async,
        is_generator: true,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

/// Residual `Expr::Yield` nodes. After the state-machine transform every
/// suspend point is a state exit; one left in the HIR is lowered by codegen
/// without suspending — the #10419 symptom (the generator yields nothing).
fn residual_yields(stmts: &[Stmt]) -> usize {
    format!("{stmts:?}").matches("Yield {").count()
}

fn transformed(body: Vec<Stmt>, is_async: bool) -> Vec<Stmt> {
    let mut module = Module::new("loop_header_yield");
    module.functions.push(generator(body, is_async));
    transform_generators(&mut module);
    std::mem::take(&mut module.functions[0].body)
}

fn if_global(then_branch: Vec<Stmt>) -> Stmt {
    Stmt::If {
        condition: Expr::GlobalGet(0),
        then_branch,
        else_branch: None,
    }
}

/// One loop per header position, each wrapped in a different container.
fn nested_header_yield_bodies() -> Vec<(&'static str, Vec<Stmt>)> {
    vec![
        (
            "if > while condition",
            vec![if_global(vec![Stmt::While {
                condition: comma_yield(1.0),
                body: vec![],
            }])],
        ),
        (
            "try/finally > do-while condition",
            vec![Stmt::Try {
                body: vec![Stmt::DoWhile {
                    body: vec![],
                    condition: comma_yield(1.0),
                }],
                catch: None,
                finally: Some(vec![]),
            }],
        ),
        (
            "catch > for condition",
            vec![Stmt::Try {
                body: vec![],
                catch: Some(CatchClause {
                    param: None,
                    body: vec![Stmt::For {
                        init: None,
                        condition: Some(comma_yield(1.0)),
                        update: None,
                        body: vec![],
                    }],
                }),
                finally: None,
            }],
        ),
        (
            "switch case > for update",
            vec![Stmt::Switch {
                discriminant: Expr::GlobalGet(0),
                cases: vec![SwitchCase {
                    test: Some(Expr::Number(1.0)),
                    body: vec![Stmt::For {
                        init: None,
                        condition: Some(Expr::GlobalGet(0)),
                        update: Some(comma_yield(1.0)),
                        body: vec![],
                    }],
                }],
            }],
        ),
        (
            "label > while condition",
            vec![Stmt::Labeled {
                label: "outer".to_string(),
                body: Box::new(Stmt::While {
                    condition: comma_yield(1.0),
                    body: vec![Stmt::LabeledContinue("outer".to_string())],
                }),
            }],
        ),
        (
            "if > for init",
            vec![if_global(vec![Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 1,
                    name: "t".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(yield_num(1.0)),
                })),
                condition: Some(Expr::GlobalGet(0)),
                update: None,
                body: vec![],
            }])],
        ),
        (
            "if > for update with continue inside try/finally",
            vec![if_global(vec![Stmt::For {
                init: None,
                condition: Some(Expr::GlobalGet(0)),
                update: Some(comma_yield(1.0)),
                body: vec![Stmt::Try {
                    body: vec![Stmt::Continue],
                    catch: None,
                    finally: Some(vec![]),
                }],
            }])],
        ),
    ]
}

#[test]
fn body_contains_yield_sees_nested_loop_headers() {
    for (name, body) in nested_header_yield_bodies() {
        // The init case is normalized by `hoist_yields_in_stmts` (the pass
        // that runs before any linearizer decision), not by header detection.
        let mut body = body;
        let mut next_id = 100;
        hoist_yields_in_stmts(&mut body, &mut next_id);
        assert!(
            body_contains_yield(&body),
            "{name}: a loop-header yield must make the enclosing statement linearize"
        );
    }
}

#[test]
fn body_contains_yield_ignores_header_yield_in_nested_closure() {
    // A `yield` inside a closure in the loop condition belongs to that closure.
    let closure = Expr::Closure {
        func_id: 9,
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Expr(yield_num(1.0))],
        captures: Vec::new(),
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: true,
        is_strict: false,
    };
    let body = vec![if_global(vec![Stmt::While {
        condition: Expr::Sequence(vec![closure, Expr::Bool(false)]),
        body: vec![],
    }])];
    assert!(!body_contains_yield(&body));
}

#[test]
fn nested_loop_header_yields_become_state_exits() {
    for is_async in [false, true] {
        for (name, body) in nested_header_yield_bodies() {
            assert!(
                residual_yields(&body) > 0,
                "{name}: fixture must start with a yield, or the check below is vacuous"
            );
            let out = transformed(body, is_async);
            assert_eq!(
                residual_yields(&out),
                0,
                "{name} (async={is_async}): header yield left unsplit: {out:?}"
            );
        }
    }
}

#[test]
fn for_init_top_level_yield_is_hoisted_before_the_loop() {
    let mut stmts = vec![Stmt::For {
        init: Some(Box::new(Stmt::Expr(yield_num(7.0)))),
        condition: Some(Expr::GlobalGet(0)),
        update: None,
        body: vec![],
    }];
    let mut next_id = 100;
    hoist_yields_in_stmts(&mut stmts, &mut next_id);
    assert!(
        matches!(
            &stmts[0],
            Stmt::Let {
                init: Some(Expr::Yield { .. }),
                ..
            }
        ),
        "init yield must be hoisted into a leading `let = yield`: {stmts:?}"
    );
    match &stmts[1] {
        Stmt::For { init: Some(i), .. } => assert_eq!(residual_yields(std::slice::from_ref(i)), 0),
        other => panic!("expected the For after the hoisted let, got {other:?}"),
    }
}

/// In an `async function*`, `yield E` awaits `E` first. The pre-linearize
/// operand pass never sees loop headers, so the linearizer must add the
/// await when it moves a header yield into the loop (`yield promise` in a
/// loop condition delivered the promise itself).
#[test]
fn async_generator_header_yield_awaits_its_operand() {
    for is_async in [false, true] {
        super::linearize::set_linearize_async_generator(is_async);
        let body = vec![Stmt::While {
            condition: comma_yield(1.0),
            body: vec![],
        }];
        let mut states = Vec::new();
        let mut current = Vec::new();
        let mut state_num = 0;
        let mut next_id = 100;
        let mut catches = Vec::new();
        let mut finallys = Vec::new();
        linearize_body(
            &body,
            &mut states,
            &mut current,
            &mut state_num,
            90,
            &mut next_id,
            91,
            &mut catches,
            &mut finallys,
        );
        super::linearize::set_linearize_async_generator(false);
        let await_state = states
            .iter()
            .find(|s| matches!(s.exit, StateExit::Await { .. }))
            .map(|s| s.num);
        let yield_state = states
            .iter()
            .find(|s| matches!(s.exit, StateExit::Yield { .. }))
            .map(|s| s.num)
            .expect("the header yield must become a Yield state");
        if is_async {
            let await_state = await_state.expect("async header yield must await its operand");
            assert!(
                await_state < yield_state,
                "operand await must precede the yield"
            );
        } else {
            assert_eq!(await_state, None, "sync generators never await");
        }
    }
}
