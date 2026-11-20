//! `collectors::contents_stable_bindings`: which bindings may keep a proof
//! about their object's contents.

use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Param, Stmt};

use super::contents_stable::contents_stable_bindings;

fn literal() -> Expr {
    Expr::Object(vec![
        ("x".to_string(), Expr::Number(1.0)),
        ("y".to_string(), Expr::Number(2.0)),
    ])
}

fn let_(id: u32, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

fn get(id: u32, property: &str) -> Expr {
    Expr::PropertyGet {
        byte_offset: 0,
        byte_end: 0,
        object: Box::new(Expr::LocalGet(id)),
        property: property.to_string(),
    }
}

fn set(id: u32, property: &str, value: Expr) -> Stmt {
    Stmt::Expr(Expr::PropertySet {
        byte_offset: 0,
        byte_end: 0,
        object: Box::new(Expr::LocalGet(id)),
        property: property.to_string(),
        value: Box::new(value),
    })
}

fn call(callee: Expr, args: Vec<Expr>) -> Expr {
    Expr::Call {
        byte_offset: 0,
        byte_end: 0,
        callee: Box::new(callee),
        args,
        type_args: Vec::new(),
    }
}

fn product(id: u32) -> Stmt {
    Stmt::Expr(Expr::Binary {
        op: perry_hir::BinaryOp::Mul,
        left: Box::new(get(id, "x")),
        right: Box::new(get(id, "y")),
    })
}

fn function(id: u32, params: Vec<u32>, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: format!("f{id}"),
        type_params: Vec::new(),
        params: params
            .into_iter()
            .map(|pid| Param {
                id: pid,
                name: format!("p{pid}"),
                ty: Type::Any,
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
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn stable(init: Vec<Stmt>, functions: Vec<Function>) -> std::collections::HashSet<u32> {
    let mut module = Module::new("contents_stable.ts");
    module.init = init;
    module.functions = functions;
    contents_stable_bindings(&module)
}

#[test]
fn a_literal_nothing_writes_is_stable() {
    let ids = stable(vec![let_(1, literal()), product(1)], Vec::new());
    assert!(ids.contains(&1));
}

#[test]
fn a_delete_or_a_store_is_a_write() {
    let delete = Stmt::Expr(Expr::Delete(Box::new(get(1, "y"))));
    assert!(!stable(vec![let_(1, literal()), delete, product(1)], Vec::new()).contains(&1));
    let store = set(1, "y", Expr::String("s".to_string()));
    assert!(!stable(vec![let_(1, literal()), store, product(1)], Vec::new()).contains(&1));
}

#[test]
fn a_write_through_an_alias_writes_every_alias() {
    let ids = stable(
        vec![
            let_(1, literal()),
            let_(2, Expr::LocalGet(1)),
            set(2, "y", Expr::String("s".to_string())),
            product(1),
        ],
        Vec::new(),
    );
    assert!(!ids.contains(&1) && !ids.contains(&2));
}

#[test]
fn a_child_handed_out_escapes_its_parent_unless_primitive() {
    let nested = Expr::Object(vec![("a".to_string(), literal())]);
    let ids = stable(vec![let_(1, nested), let_(2, get(1, "a"))], Vec::new());
    assert!(!ids.contains(&1));
    let ids = stable(vec![let_(1, literal()), let_(2, get(1, "x"))], Vec::new());
    assert!(ids.contains(&1));
}

#[test]
fn a_call_argument_escapes_unless_the_callee_parameter_is_stable() {
    // f10(p11) writes its parameter; f20(p21) only reads it.
    let writer = function(
        10,
        vec![11],
        vec![set(11, "y", Expr::String("s".to_string()))],
    );
    let reader = function(20, vec![21], vec![Stmt::Return(Some(get(21, "x")))]);
    let ids = stable(
        vec![
            let_(1, literal()),
            Stmt::Expr(call(Expr::FuncRef(10), vec![Expr::LocalGet(1)])),
            let_(2, literal()),
            Stmt::Expr(call(Expr::FuncRef(20), vec![Expr::LocalGet(2)])),
            let_(3, literal()),
            Stmt::Expr(call(Expr::GlobalGet(0), vec![Expr::LocalGet(3)])),
        ],
        vec![writer, reader],
    );
    assert!(!ids.contains(&1), "a writing callee");
    assert!(ids.contains(&2), "a read-only callee");
    assert!(ids.contains(&21), "a read-only parameter");
    assert!(!ids.contains(&3), "an unknown callee");
}

#[test]
fn a_parameter_needs_a_body_without_unknown_code_or_foreign_stores() {
    let calls = function(
        10,
        vec![11],
        vec![
            Stmt::Expr(call(Expr::GlobalGet(0), Vec::new())),
            Stmt::Return(Some(get(11, "x"))),
        ],
    );
    // `other.y = v` may be a store through the same object (`f(o, o)`).
    let foreign = function(
        20,
        vec![21, 22],
        vec![set(22, "y", Expr::Number(1.0)), product(21)],
    );
    let fresh_store = function(
        30,
        vec![31],
        vec![
            let_(32, literal()),
            set(32, "y", Expr::Number(1.0)),
            product(31),
        ],
    );
    let ids = stable(Vec::new(), vec![calls, foreign, fresh_store]);
    assert!(!ids.contains(&11));
    assert!(!ids.contains(&21));
    assert!(
        ids.contains(&31),
        "a store into a fresh local cannot reach the parameter"
    );
}

#[test]
fn a_closure_write_is_a_write() {
    let closure = Expr::Closure {
        func_id: 50,
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![set(1, "y", Expr::String("s".to_string()))],
        captures: vec![1],
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    };
    let ids = stable(
        vec![let_(1, literal()), let_(2, closure), product(1)],
        Vec::new(),
    );
    assert!(!ids.contains(&1));
}

#[test]
fn a_callee_returning_an_object_child_hands_it_out() {
    // f10(p11) returns p11.a (an object), f20(p21) returns p21.x (a number).
    let object_child = function(10, vec![11], vec![Stmt::Return(Some(get(11, "a")))]);
    let number_child = function(20, vec![21], vec![Stmt::Return(Some(get(21, "x")))]);
    let nested = Expr::Object(vec![
        ("a".to_string(), literal()),
        ("x".to_string(), Expr::Number(1.0)),
    ]);
    let ids = stable(
        vec![
            let_(1, nested.clone()),
            Stmt::Expr(call(Expr::FuncRef(10), vec![Expr::LocalGet(1)])),
            let_(2, nested),
            Stmt::Expr(call(Expr::FuncRef(20), vec![Expr::LocalGet(2)])),
        ],
        vec![object_child, number_child],
    );
    assert!(!ids.contains(&1), "an object child handed back");
    assert!(ids.contains(&2), "a number child handed back");
}
