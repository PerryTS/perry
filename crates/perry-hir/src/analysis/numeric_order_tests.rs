use super::*;
use crate::types::Type;
fn local(id: u32) -> Expr {
    Expr::LocalGet(id)
}
fn num(n: f64) -> Expr {
    Expr::Number(n)
}
fn compare(op: CompareOp, a: Expr, b: Expr) -> Expr {
    Expr::Compare {
        op,
        left: Box::new(a),
        right: Box::new(b),
    }
}
fn sub(a: Expr, b: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Sub,
        left: Box::new(a),
        right: Box::new(b),
    }
}
fn ternary(c: Expr, a: Expr, b: Expr) -> Expr {
    Expr::Conditional {
        condition: Box::new(c),
        then_expr: Box::new(a),
        else_expr: Box::new(b),
    }
}
fn key(id: u32, property: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(local(id)),
        property: property.into(),
        byte_offset: 0,
        byte_end: 0,
    }
}
fn call(callee: Expr, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(callee),
        args,
        type_args: vec![],
        byte_offset: 0,
        byte_end: 0,
    }
}
fn callback(body: Vec<Stmt>) -> Expr {
    Expr::Closure {
        func_id: 99,
        params: [17, 23]
            .into_iter()
            .map(|id| Param {
                id,
                name: format!("p{id}"),
                ty: Type::Any,
                default: None,
                decorators: vec![],
                is_rest: false,
                arguments_object: None,
            })
            .collect(),
        return_type: Type::Any,
        body,
        captures: vec![],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    }
}
fn ret(e: Expr) -> Vec<Stmt> {
    vec![Stmt::Return(Some(e))]
}
fn branches() -> Vec<Stmt> {
    vec![
        Stmt::If {
            condition: compare(CompareOp::Lt, local(17), local(23)),
            then_branch: ret(num(-1.0)),
            else_branch: None,
        },
        Stmt::If {
            condition: compare(CompareOp::Gt, local(17), local(23)),
            then_branch: ret(num(1.0)),
            else_branch: None,
        },
        Stmt::Return(Some(num(0.0))),
    ]
}
#[test]
fn sign_proof_accepts_control_flow_coercions_aliases_and_both_directions() {
    let tern = ternary(
        compare(CompareOp::Lt, local(17), local(23)),
        num(-1.0),
        ternary(
            compare(CompareOp::Gt, local(17), local(23)),
            num(1.0),
            num(0.0),
        ),
    );
    for expr in [
        sub(local(17), local(23)),
        tern,
        sub(
            Expr::NumberCoerce(Box::new(local(17))),
            Expr::NumberCoerce(Box::new(local(23))),
        ),
        sub(
            Expr::Unary {
                op: UnaryOp::Pos,
                operand: Box::new(local(17)),
            },
            local(23),
        ),
    ] {
        assert_eq!(
            pure_numeric_order(&callback(ret(expr.clone()))),
            Some(NumericOrder::Ascending)
        );
        assert_eq!(
            pure_numeric_order(&callback(ret(Expr::Unary {
                op: UnaryOp::Neg,
                operand: Box::new(expr)
            }))),
            Some(NumericOrder::Descending)
        );
    }
    assert_eq!(
        pure_numeric_order(&callback(branches())),
        Some(NumericOrder::Ascending)
    );
    let mut body = vec![Stmt::Let {
        id: 50,
        name: "d".into(),
        ty: Type::String,
        mutable: false,
        init: Some(sub(local(17), local(23))),
    }];
    body.extend(ret(local(50)));
    assert_eq!(
        pure_numeric_order(&callback(body)),
        Some(NumericOrder::Ascending)
    );
}
#[test]
fn sign_proof_closes_helpers_and_projection_bodies_transitively() {
    let helper = callback(branches());
    let project = Expr::Closure {
        func_id: 100,
        params: vec![Param {
            id: 17,
            name: "x".into(),
            ty: Type::Any,
            default: None,
            decorators: vec![],
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Any,
        body: ret(key(17, "rank")),
        captures: vec![],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    };
    let mut outer = callback(ret(call(local(500), vec![local(17), local(23)])));
    if let Expr::Closure {
        func_id, captures, ..
    } = &mut outer
    {
        *func_id = 101;
        captures.push(500);
    }
    assert_eq!(
        numeric_order_with(&outer, &|e| match e {
            Expr::LocalGet(500) => NumericCallable::closure(&helper),
            _ => None,
        }),
        Some(NumericOrder::Ascending)
    );
    let outer = callback(ret(sub(
        call(local(500), vec![local(17)]),
        call(local(500), vec![local(23)]),
    )));
    assert_eq!(
        numeric_order_with(&outer, &|e| match e {
            Expr::LocalGet(500) => NumericCallable::closure(&project),
            _ => None,
        }),
        Some(NumericOrder::Keyed {
            property: "rank".into(),
            descending: false
        })
    );
    assert_eq!(pure_numeric_order(&outer), None); // TOP / absent resolver
}
#[test]
fn sign_proof_refuses_inconsistency_effects_captures_and_mixed_domains() {
    for body in [
        ret(ternary(
            compare(CompareOp::Lt, local(17), local(23)),
            num(-1.0),
            num(1.0),
        )),
        ret(sub(key(17, "a"), key(23, "b"))),
        ret(sub(local(17), local(500))),
        ret(call(local(500), vec![local(17), local(23)])),
        vec![
            Stmt::Expr(Expr::LocalSet(17, Box::new(num(0.0)))),
            Stmt::Return(Some(sub(local(17), local(23)))),
        ],
        ret(ternary(
            compare(CompareOp::Eq, local(17), local(23)),
            num(-1.0),
            sub(local(17), local(23)),
        )),
    ] {
        assert_eq!(pure_numeric_order(&callback(body)), None);
    }
    let mut bad = branches();
    bad[2] = Stmt::Return(Some(num(1.0)));
    assert_eq!(pure_numeric_order(&callback(bad)), None);
}
#[test]
fn nan_from_equal_infinities_cannot_be_assumed_equal_to_zero() {
    let d = sub(local(17), local(23));
    // At equal infinities, d === 0 is false; this comparator returns 1.
    let c = callback(ret(ternary(
        compare(CompareOp::Eq, d.clone(), num(0.0)),
        num(0.0),
        ternary(compare(CompareOp::Lt, d, num(0.0)), num(-1.0), num(1.0)),
    )));
    assert_eq!(pure_numeric_order(&c), None);
}
#[test]
fn recursive_or_effectful_resolved_helpers_are_not_pure() {
    let helper = callback(ret(call(local(500), vec![local(17), local(23)])));
    let mut outer = callback(ret(call(local(500), vec![local(17), local(23)])));
    if let Expr::Closure { func_id, .. } = &mut outer {
        *func_id = 102;
    }
    assert_eq!(
        numeric_order_with(&outer, &|_| NumericCallable::closure(&helper)),
        None
    );
    let helper = callback(vec![
        Stmt::Expr(Expr::LocalSet(7, Box::new(num(3.0)))),
        Stmt::Return(Some(sub(local(17), local(23)))),
    ]);
    assert_eq!(
        numeric_order_with(&outer, &|_| NumericCallable::closure(&helper)),
        None
    );
}

#[test]
fn optimized_integer_constants_have_the_same_numeric_meaning() {
    let body = vec![
        Stmt::If {
            condition: compare(CompareOp::Lt, local(17), local(23)),
            then_branch: ret(Expr::Integer(-37)),
            else_branch: None,
        },
        Stmt::If {
            condition: compare(CompareOp::Gt, local(17), local(23)),
            then_branch: ret(Expr::Integer(42)),
            else_branch: None,
        },
        Stmt::Return(Some(Expr::Integer(0))),
    ];
    assert_eq!(
        pure_numeric_order(&callback(body)),
        Some(NumericOrder::Ascending)
    );
}

#[test]
fn signed_zero_joins_do_not_invent_an_exact_reciprocal_sign() {
    let zero = ternary(
        compare(CompareOp::Lt, local(17), num(0.0)),
        num(-0.0),
        num(0.0),
    );
    let reciprocal = Expr::Binary {
        op: BinaryOp::Div,
        left: Box::new(num(1.0)),
        right: Box::new(zero),
    };
    let body = vec![
        Stmt::Let {
            id: 50,
            name: "scale".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(reciprocal),
        },
        Stmt::Return(Some(ternary(
            compare(CompareOp::Lt, local(17), local(23)),
            Expr::Unary {
                op: UnaryOp::Neg,
                operand: Box::new(local(50)),
            },
            ternary(
                compare(CompareOp::Gt, local(17), local(23)),
                local(50),
                num(0.0),
            ),
        ))),
    ];
    assert_eq!(pure_numeric_order(&callback(body)), None);
}

#[test]
fn helper_fallthrough_and_bare_return_remain_undefined_until_the_consumer() {
    for bare_return in [false, true] {
        let mut body = branches();
        body.pop();
        if bare_return {
            body.push(Stmt::Return(None));
        }
        let helper = callback(body);
        let mut direct = callback(ret(call(local(500), vec![local(17), local(23)])));
        if let Expr::Closure { func_id, .. } = &mut direct {
            *func_id = 101;
        }
        let resolve = |e: &Expr| {
            if matches!(e, Expr::LocalGet(500)) {
                NumericCallable::closure(&helper)
            } else {
                None
            }
        };
        assert_eq!(
            numeric_order_with(&direct, &resolve),
            Some(NumericOrder::Ascending)
        );
        let invocation = call(local(500), vec![local(17), local(23)]);
        let mut inconsistent = callback(ret(ternary(
            compare(CompareOp::Eq, invocation.clone(), Expr::Undefined),
            num(1.0),
            invocation,
        )));
        if let Expr::Closure { func_id, .. } = &mut inconsistent {
            *func_id = 102;
        }
        assert_eq!(numeric_order_with(&inconsistent, &resolve), None);
    }
}
#[test]
fn strict_boolean_number_equality_is_not_numeric_coercion() {
    let less = compare(CompareOp::Lt, local(17), local(23));
    let tail = ternary(
        compare(CompareOp::Gt, local(17), local(23)),
        num(1.0),
        num(0.0),
    );
    let inconsistent = callback(ret(ternary(
        compare(CompareOp::Eq, less.clone(), num(0.0)),
        tail.clone(),
        num(-1.0),
    )));
    assert_eq!(pure_numeric_order(&inconsistent), None);
    let numeric = callback(ret(ternary(
        compare(CompareOp::Eq, Expr::NumberCoerce(Box::new(less)), num(0.0)),
        tail,
        num(-1.0),
    )));
    assert_eq!(pure_numeric_order(&numeric), Some(NumericOrder::Ascending));
}
