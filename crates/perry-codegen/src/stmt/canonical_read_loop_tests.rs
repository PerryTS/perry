use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

fn read(key: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(2)),
        property: key.into(),
        byte_offset: 0,
    }
}
fn add(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}
fn reduction() -> Stmt {
    Stmt::Expr(Expr::LocalSet(
        3,
        Box::new(add(Expr::LocalGet(3), add(read("a"), read("b")))),
    ))
}
fn probe(mut prefix: Vec<Stmt>) -> String {
    let allocating = prefix
        .iter()
        .any(|s| matches!(s, Stmt::Expr(Expr::LocalSet(5, _))));
    prefix.push(reduction());
    let mut m = Module::new("canonical_read_probe");
    m.functions.push(Function {
        id: 1,
        name: "run".into(),
        type_params: vec![],
        params: (1..=2)
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
        body: vec![
            Stmt::Let {
                id: 5,
                name: "scratch".into(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Array(vec![])),
            },
            Stmt::Let {
                id: 3,
                name: "h".into(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 4,
                    name: "k".into(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(4)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 4,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: prefix,
            },
            Stmt::Return(Some(if allocating {
                Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(5)),
                    index: Box::new(Expr::Integer(0)),
                }
            } else {
                Expr::LocalGet(3)
            })),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    String::from_utf8(
        compile_module(
            &m,
            CompileOptions {
                emit_ir_only: true,
                is_entry_module: false,
                ..CompileOptions::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn canonical_read_loop_has_one_shape_guard_and_constant_slots() {
    let ir = probe(vec![]);
    assert!(
        ir.contains("for.shape_read.body"),
        "fast loop must be live: {ir}"
    );
    assert!(
        ir.contains("call i32 @js_canonical_read_shape"),
        "canonical supplier missing: {ir}"
    );
    assert!(
        ir.contains("call double @js_object_get_field_generic"),
        "one generic miss required: {ir}"
    );
    assert!(
        !ir.contains("call i64 @js_region_guard_prime"),
        "no learned region in either arm: {ir}"
    );
    assert!(
        !ir.contains("_packed_get ="),
        "no second per-site fast path: {ir}"
    );
    let body = ir
        .split("for.shape_read.body")
        .find(|s| s.contains("load double"));
    assert!(body.is_some(), "body must load slots: {ir}");
    // The expected shape's key positions are constants, never bit fields.
    assert!(
        !ir.lines()
            .any(|l| l.contains("lshr i64") && (l.ends_with(", 32") || l.ends_with(", 38"))),
        "slot decode remains: {ir}"
    );
}

#[test]
fn canonical_read_loop_refuses_receiver_store() {
    let store = Expr::PropertySet {
        object: Box::new(Expr::LocalGet(2)),
        property: "c".into(),
        value: Box::new(Expr::Integer(1)),
    };
    let ir = probe(vec![Stmt::Expr(store)]);
    assert!(
        !ir.contains("shape_loop.fast"),
        "layout-changing store admitted: {ir}"
    );
}

#[test]
fn canonical_read_loop_refuses_calls() {
    let call = Expr::Call {
        callee: Box::new(Expr::LocalGet(2)),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    };
    let ir = probe(vec![Stmt::Expr(call)]);
    assert!(
        !ir.contains("shape_loop.fast"),
        "unknown callback admitted: {ir}"
    );
}

#[test]
fn canonical_read_loop_refuses_receiver_reassignment() {
    let ir = probe(vec![Stmt::Expr(Expr::LocalSet(
        2,
        Box::new(Expr::Integer(5)),
    ))]);
    assert!(
        !ir.contains("shape_loop.fast"),
        "receiver reassignment admitted: {ir}"
    );
}

#[test]
fn canonical_read_loop_allocation_keeps_the_guard_and_poll_live() {
    let ir = probe(vec![Stmt::Expr(Expr::LocalSet(
        5,
        Box::new(Expr::Array(vec![Expr::LocalGet(4)])),
    ))]);
    assert!(
        ir.contains("for.shape_read.body"),
        "allocating loop must enter the guarded arm: {ir}"
    );
    assert!(
        ir.contains("call void @js_gc_loop_safepoint"),
        "a moving poll must remain: {ir}"
    );
}

#[test]
fn canonical_read_loop_array_initializers_cannot_call_user_code() {
    let call = Expr::Call {
        callee: Box::new(Expr::LocalGet(2)),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    };
    let ir = probe(vec![Stmt::Expr(Expr::LocalSet(
        5,
        Box::new(Expr::Array(vec![call])),
    ))]);
    assert!(
        !ir.contains("shape_loop.fast"),
        "array initializer callback admitted: {ir}"
    );
}
