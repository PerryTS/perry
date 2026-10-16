//! A closure's capture list holds its FREE references only: a binding that a
//! nested closure declares belongs to that closure, however deep it is used.

use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Param, Stmt};

use super::compute_auto_captures_with_globals;

fn param(id: u32) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn closure(func_id: u32, params: Vec<Param>, body: Vec<Stmt>, captures: Vec<u32>) -> Expr {
    Expr::Closure {
        func_id,
        params,
        return_type: Type::Any,
        body,
        captures,
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    }
}

fn let_(id: u32, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: Type::Any,
        mutable: true,
        init: Some(init),
    }
}

fn add(a: u32, b: u32) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::LocalGet(a)),
        right: Box::new(Expr::LocalGet(b)),
    }
}

fn captures_of(params: &[Param], body: &[Stmt], explicit: &[u32]) -> Vec<u32> {
    compute_auto_captures_with_globals(
        params,
        body,
        explicit,
        &std::collections::HashMap::new(),
        &crate::scope_env::ScopeMap::default(),
    )
}

/// `() => { const f = (p2) => { let b = p2; const g = () => b + a; return g; }; return f; }`:
/// the outer closure needs `a` (id 1) only. `p2`, `b` and `g` are bindings of
/// the nested closures and used to leak into every enclosing capture list.
#[test]
fn nested_closure_bindings_are_not_captured_by_an_enclosing_closure() {
    let g = closure(
        12,
        Vec::new(),
        vec![Stmt::Return(Some(add(3, 1)))],
        vec![3, 1],
    );
    let f_body = vec![
        let_(3, Expr::LocalGet(2)),
        let_(4, g),
        Stmt::Return(Some(Expr::LocalGet(4))),
    ];
    // The inner closure's explicit list is empty here (the shape HIR leaves
    // for some argument closures): its free references must still surface.
    let f = closure(11, vec![param(2)], f_body, Vec::new());
    let outer_body = vec![let_(5, f), Stmt::Return(Some(Expr::LocalGet(5)))];
    assert_eq!(captures_of(&[], &outer_body, &[]), vec![1]);
}

/// A reference to an id bound in a nested closure still counts when the
/// enclosing body ALSO reads it at its own level, and a nested closure's
/// explicit capture list is always forwarded.
#[test]
fn own_level_references_and_explicit_nested_captures_are_kept() {
    let g = closure(
        21,
        vec![param(7)],
        vec![Stmt::Return(Some(add(7, 8)))],
        vec![9],
    );
    let body = vec![let_(10, g), Stmt::Return(Some(add(10, 7)))];
    assert_eq!(captures_of(&[], &body, &[]), vec![7, 8, 9]);
}
