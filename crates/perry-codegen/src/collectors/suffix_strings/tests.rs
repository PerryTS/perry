use super::*;
use perry_hir::types::Type;

fn property(name: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(1)),
        property: name.into(),
        byte_offset: 0,
    }
}
fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(property(name)),
        args,
        type_args: vec![],
        byte_offset: 0,
    }
}
fn assign(args: Vec<Expr>) -> Expr {
    Expr::LocalSet(1, Box::new(call("slice", args)))
}
fn body() -> Vec<Stmt> {
    vec![
        Stmt::Let {
            id: 1,
            name: "s".into(),
            ty: Type::String,
            mutable: true,
            init: Some(Expr::String("😀abc".into())),
        },
        Stmt::While {
            condition: property("length"),
            body: vec![
                Stmt::Expr(call("charCodeAt", vec![Expr::Integer(0)])),
                Stmt::Expr(assign(vec![Expr::Integer(1)])),
            ],
        },
    ]
}
fn selected(stmts: &[Stmt]) -> bool {
    collect(stmts, &HashSet::new(), &HashMap::new()).contains(&1)
}

#[test]
fn accepts_scalar_consumption_with_utf16_strides() {
    assert!(selected(&body()));
    for count in [0, 2, 5, i32::MAX] {
        let mut stmts = body();
        stmts.push(Stmt::Expr(assign(vec![Expr::Integer(count as i64)])));
        stmts.push(Stmt::Return(Some(property("length"))));
        assert!(selected(&stmts));
    }
}

#[test]
fn leaves_unrelated_and_immutable_locals_alone() {
    assert!(!selected(&body()[..1]));
    let mut stmts = body();
    let Stmt::Let { mutable, .. } = &mut stmts[0] else {
        unreachable!();
    };
    *mutable = false;
    assert!(!selected(&stmts));
}

#[test]
fn rejects_escapes_aliases_captures_and_unsupported_updates() {
    let cases = [
        Stmt::Return(Some(Expr::LocalGet(1))),
        Stmt::Let {
            id: 2,
            name: "alias".into(),
            ty: Type::String,
            mutable: false,
            init: Some(Expr::LocalGet(1)),
        },
        Stmt::Return(Some(assign(vec![Expr::Integer(1)]))),
        Stmt::Expr(assign(vec![Expr::Integer(-1)])),
        Stmt::Expr(assign(vec![Expr::Integer(1), Expr::Integer(2)])),
        Stmt::Expr(assign(vec![Expr::LocalGet(3)])),
        Stmt::Expr(call("charCodeAt", vec![Expr::LocalGet(3)])),
        Stmt::Expr(call("toString", vec![])),
        Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::String("changed".into())))),
    ];
    for extra in cases {
        let mut stmts = body();
        stmts.push(extra.clone());
        assert!(!selected(&stmts), "must reject {extra:?}");
    }
    assert!(collect(&body(), &HashSet::from([1]), &HashMap::new()).is_empty());
    assert!(collect(
        &body(),
        &HashSet::new(),
        &HashMap::from([(1, "global".into())])
    )
    .is_empty());
}
