use super::*;

// Sanity tests at the helper level. End-to-end tests live in
// test-files/test_deforest_*.ts (compiled + run vs Node).

#[test]
fn detects_simple_producer() {
    // function f() { const out = []; out.push(1); return out; }
    let func = Function {
        id: 1,
        name: "f".to_string(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Array(Box::new(Type::Number)),
        body: vec![
            Stmt::Let {
                id: 10,
                name: "out".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: false,
                init: Some(Expr::Array(vec![])),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: 10,
                value: Box::new(Expr::Integer(1)),
            }),
            Stmt::Return(Some(Expr::LocalGet(10))),
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    };
    let info = analyze_producer(&func).expect("should detect producer");
    assert_eq!(info.out_local_id, 10);
    assert_eq!(info.original_param_count, 0);
    assert!(matches!(info.elem_ty, Type::Number));
}

#[test]
fn rejects_async_producer() {
    let mut func = make_simple_producer();
    func.is_async = true;
    assert!(analyze_producer(&func).is_none());
}

#[test]
fn rejects_producer_with_out_passed_to_call() {
    // function f() { const out = []; helper(out); return out; }
    // Passing `out` to `helper` is unsafe — it might escape.
    let mut func = make_simple_producer();
    // Replace the push with `helper(out)`.
    func.body[1] = Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(99)),
        args: vec![Expr::LocalGet(10)],
        type_args: vec![],
    });
    assert!(analyze_producer(&func).is_none());
}

#[test]
fn rejects_producer_with_reassignment() {
    // function f() { const out = []; out = [1, 2]; return out; }
    let mut func = make_simple_producer();
    func.body[1] = Stmt::Expr(Expr::LocalSet(
        10,
        Box::new(Expr::Array(vec![Expr::Integer(1)])),
    ));
    assert!(analyze_producer(&func).is_none());
}

#[test]
fn rejects_producer_with_multiple_returns() {
    // function f(cond) { const out = []; if (cond) return []; return out; }
    let mut func = make_simple_producer();
    func.body.insert(
        1,
        Stmt::If {
            condition: Expr::Bool(true),
            then_branch: vec![Stmt::Return(Some(Expr::Array(vec![])))],
            else_branch: None,
        },
    );
    assert!(analyze_producer(&func).is_none());
}

fn make_simple_producer() -> Function {
    Function {
        id: 1,
        name: "f".to_string(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Array(Box::new(Type::Number)),
        body: vec![
            Stmt::Let {
                id: 10,
                name: "out".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: false,
                init: Some(Expr::Array(vec![])),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: 10,
                value: Box::new(Expr::Integer(1)),
            }),
            Stmt::Return(Some(Expr::LocalGet(10))),
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    }
}
