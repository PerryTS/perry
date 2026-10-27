use super::*;
fn init() -> Stmt {
    Stmt::Let {
        id: 2,
        name: "m".into(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "exec".into(),
                byte_offset: 0,
            }),
            args: vec![Expr::String("input".into())],
            type_args: vec![],
            byte_offset: 0,
        }),
    }
}
fn get(n: i64) -> Expr {
    Expr::IndexGet {
        object: Box::new(Expr::LocalGet(2)),
        index: Box::new(Expr::Integer(n)),
    }
}
fn optimized(extra: Vec<Stmt>) -> Module {
    let mut module = Module::new("projection");
    let mut body = vec![init()];
    body.extend(extra);
    module.init.push(Stmt::While {
        condition: Expr::Bool(false),
        body,
    });
    run(&mut module);
    module
}
fn projected(module: &Module) -> bool {
    format!("{:?}", module.init).contains("js_regexp_exec_projected")
}
#[test]
fn projects_repeated_capture_and_null_check() {
    let m = optimized(vec![
        Stmt::Expr(Expr::Compare {
            op: CompareOp::Ne,
            left: Box::new(Expr::LocalGet(2)),
            right: Box::new(Expr::Null),
        }),
        Stmt::Expr(get(7)),
        Stmt::Expr(get(7)),
    ]);
    assert!(projected(&m));
    let Stmt::While { body, .. } = &m.init[0] else {
        panic!()
    };
    let Stmt::Let {
        init: Some(Expr::Call { args, .. }),
        ..
    } = &body[0]
    else {
        panic!()
    };
    assert!(
        matches!(args.as_slice(), [Expr::LocalGet(1), Expr::PropertyGet { property, .. }, Expr::String(_), Expr::Number(7.0)] if property == "exec")
    );
}
#[test]
fn rejects_escapes_metadata_aliases_writes_and_multiple_captures() {
    for extra in [
        Stmt::Return(Some(Expr::LocalGet(2))),
        Stmt::Expr(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(2)),
            property: "index".into(),
            byte_offset: 0,
        }),
        Stmt::Let {
            id: 3,
            name: "alias".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::LocalGet(2)),
        },
        Stmt::Expr(Expr::LocalSet(2, Box::new(Expr::Null))),
        Stmt::Expr(get(6)),
        Stmt::Expr(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(2)),
            index: Box::new(Expr::LocalGet(4)),
        }),
        Stmt::Expr(Expr::ArrayPop(2)),
        Stmt::Expr(Expr::Delete(Box::new(get(7)))),
    ] {
        assert!(!projected(&optimized(vec![Stmt::Expr(get(7)), extra])));
    }
}
#[test]
fn rejects_reference_outside_declaring_block_and_global_results() {
    let mut m = optimized(vec![Stmt::Expr(get(7))]);
    // Restart from unmodified HIR with another reference outside the block.
    if let Stmt::While { body, .. } = &mut m.init[0] {
        body[0] = init();
    }
    m.init.push(Stmt::Expr(Expr::LocalGet(2)));
    run(&mut m);
    assert!(!projected(&m));
    m.init = vec![init(), Stmt::Expr(get(7))];
    run(&mut m);
    assert!(!projected(&m));
}

#[test]
fn rejects_implicit_local_carriers() {
    for extra in [
        Stmt::Expr(Expr::ScopedTemp {
            id: 2,
            value: Box::new(Expr::Null),
            body: Box::new(Expr::Undefined),
        }),
        Stmt::Expr(Expr::WithSet {
            object: Box::new(Expr::LocalGet(9)),
            property: "m".into(),
            value: Box::new(Expr::Null),
            fallback: perry_hir::WithSetFallback::Local(2),
            strict: false,
        }),
    ] {
        assert!(!projected(&optimized(vec![Stmt::Expr(get(7)), extra])));
    }
}

#[test]
fn rejects_function_capture_metadata_even_without_localget() {
    let mut module = optimized(vec![Stmt::Expr(get(7))]);
    if let Stmt::While { body, .. } = &mut module.init[0] {
        body[0] = init();
    }
    module.functions.push(Function {
        id: 10,
        name: "closure".into(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Any,
        body: vec![],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: vec![2],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    run(&mut module);
    assert!(!projected(&module));
}
