//! Semantic inputs ported from the deleted array-region tests.
use super::class_field_barrier_tests::ir_opts;
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt, UnaryOp, UpdateOp,
};

const A: u32 = 1;
const B: u32 = 2;
const N: u32 = 3;
const F: u32 = 4;
const I: u32 = 5;

fn opts() -> CompileOptions {
    let mut o = ir_opts();
    o.is_entry_module = false;
    o
}

fn param(id: u32, name: &str, ty: Type) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn at(arr: u32) -> Expr {
    Expr::IndexGet {
        byte_offset: 0,
        byte_end: 0,
        object: Box::new(Expr::LocalGet(arr)),
        index: Box::new(Expr::LocalGet(I)),
    }
}

/// `arr[i] = value` (the `PutValueSet` form assignment statements lower to).
fn set(arr: u32, value: Expr) -> Stmt {
    Stmt::Expr(Expr::PutValueSet {
        target: Box::new(Expr::LocalGet(arr)),
        key: Box::new(Expr::LocalGet(I)),
        value: Box::new(value),
        receiver: Box::new(Expr::LocalGet(arr)),
        strict: false,
        byte_offset: 0,
        byte_end: 0,
    })
}

fn add(l: Expr, r: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(l),
        right: Box::new(r),
    }
}

/// `a[i] = a[i] + b[i]; if (a[i] > 100) b[i] = -b[i];` — two statements, a
/// branch, three element reads and two element stores.
fn physics_body() -> Vec<Stmt> {
    vec![
        set(A, add(at(A), at(B))),
        Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Gt,
                left: Box::new(at(A)),
                right: Box::new(Expr::Number(100.0)),
            },
            then_branch: vec![set(
                B,
                Expr::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(at(B)),
                },
            )],
            else_branch: None,
        },
    ]
}

/// `function probe(a, b, n, f) { for (let i = 0; i < n; i++) { body } return 0; }`
/// with `a`/`b` of type `elem`, and `update` replacing `i++` when given.
fn probe_ir(name: &str, elem: Type, body: Vec<Stmt>, update: Option<Expr>) -> String {
    let m = probe_module(name, elem, body, update);
    String::from_utf8(compile_module(&m, opts()).expect("module compiles")).expect("UTF-8 IR")
}

fn probe_module(name: &str, elem: Type, body: Vec<Stmt>, update: Option<Expr>) -> Module {
    let mut m = Module::new(name);
    m.functions = vec![Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: vec![
            param(A, "a", elem.clone()),
            param(B, "b", elem),
            param(N, "n", Type::Number),
            param(F, "f", Type::Any),
        ],
        return_type: Type::Number,
        body: vec![
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: I,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(I)),
                    right: Box::new(Expr::LocalGet(N)),
                }),
                update: Some(update.unwrap_or(Expr::Update {
                    id: I,
                    op: UpdateOp::Increment,
                    prefix: false,
                })),
                body,
            },
            Stmt::Return(Some(Expr::Integer(0))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn number_array() -> Type {
    Type::Array(Box::new(Type::Number))
}

/// The probe function's text, split into (label, lines) blocks.
fn masked(arr: u32) -> Expr {
    Expr::IndexGet {
        byte_offset: 0,
        byte_end: 0,
        object: Box::new(Expr::LocalGet(arr)),
        index: Box::new(Expr::Binary {
            op: BinaryOp::BitAnd,
            left: Box::new(Expr::LocalGet(I)),
            right: Box::new(Expr::Integer(63)),
        }),
    }
}

/// An array the loop only reads element VALUES from (`const o = a[i & 63];
/// f(o)`) in a body that calls out is no region array: the region would save
/// one load per read and re-check its guard on every iteration. A read a
/// Number consumer takes makes it one, and so does the value read in a body
/// that runs no JS (`const o = a[i & 63]; o.d = i`), where the facts hold
/// across iterations.

fn check(ir: &str) {
    crate::testing::verify_ir(ir, "array_layout_loops").expect("valid checked lowering");
    for deleted in [
        "rloop.arr.",
        "@PERRY_TA_VIEW_GUARD",
        "@PERRY_ARRAY_ITERATION_NOT_PRISTINE",
        "@js_array_has_named_properties",
    ] {
        assert!(
            !ir.contains(deleted),
            "deleted array veto/region: {deleted}"
        );
    }
    assert!(
        ir.contains("define double @perry_fn_"),
        "the semantic input must emit a body"
    );
}
#[test]
fn multistatement_numeric_array_loop_keeps_element_stores() {
    let ir = probe_ir("rarr_physics", number_array(), physics_body(), None);
    check(&ir);
    assert!(ir.contains("store double"), "element writes remain");
}
#[test]
fn float64_reads_canonicalize_nan_without_array_regions() {
    let ir = probe_ir(
        "rarr_typed",
        Type::Named("Float64Array".into()),
        physics_body(),
        None,
    );
    check(&ir);
    assert!(
        ir.contains("fcmp ord double"),
        "typed NaN payloads are canonicalized"
    );
    check(&probe_ir(
        "rarr_plain",
        number_array(),
        physics_body(),
        None,
    ));
}
#[test]
fn typed_and_unknown_receiver_loops_have_defined_symbols() {
    for (name, ty) in [
        ("rarr_typed_verify", Type::Named("Float64Array".into())),
        ("rarr_untyped_verify", Type::Any),
    ] {
        check(&probe_ir(name, ty, physics_body(), None));
    }
}
#[test]
fn integer_float_and_buffer_copy_inputs_keep_checked_lowering() {
    for kind in [
        "Int32Array",
        "Uint8Array",
        "Float32Array",
        "Buffer",
        "Float64Array",
    ] {
        check(&probe_ir(
            "rarr_copy",
            Type::Named(kind.into()),
            vec![set(A, at(B))],
            None,
        ));
    }
    check(&probe_ir(
        "rarr_copy_any",
        Type::Any,
        vec![set(A, at(B))],
        None,
    ));
}
#[test]
fn user_call_between_element_operations_has_one_loop_body() {
    let mut body = physics_body();
    body.insert(
        1,
        Stmt::Expr(Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::LocalGet(F)),
            args: vec![Expr::LocalGet(A)],
            type_args: vec![],
            byte_offset: 0,
        }),
    );
    let ir = probe_ir("rarr_call", number_array(), body, None);
    check(&ir);
    assert!(
        !ir.contains("rloop.fast"),
        "no stale array loop clone across the call"
    );
    assert!(
        ir.contains("native_call") || ir.contains("js_call"),
        "user call remains observable"
    );
}
#[test]
fn body_and_update_writes_to_counter_or_bound_remain_observable() {
    let mut body = physics_body();
    body.push(Stmt::Expr(Expr::Update {
        id: I,
        op: UpdateOp::Increment,
        prefix: false,
    }));
    check(&probe_ir("rarr_counter_body", number_array(), body, None));
    let update = Expr::Sequence(vec![
        Expr::Update {
            id: I,
            op: UpdateOp::Increment,
            prefix: false,
        },
        Expr::LocalSet(N, Box::new(add(Expr::LocalGet(N), Expr::Integer(1)))),
    ]);
    check(&probe_ir(
        "rarr_bound_update",
        number_array(),
        physics_body(),
        Some(update),
    ));
    let mut body = physics_body();
    body.push(Stmt::Expr(Expr::LocalSet(
        N,
        Box::new(add(Expr::LocalGet(N), Expr::Integer(1))),
    )));
    check(&probe_ir("rarr_bound_body", number_array(), body, None));
}
#[test]
fn a_string_store_does_not_borrow_a_numeric_store_proof() {
    let ir = probe_ir(
        "rarr_string_store",
        number_array(),
        vec![
            set(B, add(at(B), Expr::Integer(1))),
            set(A, Expr::String("s".into())),
        ],
        None,
    );
    check(&ir);
    assert!(
        ir.contains("index_set") || ir.contains("put_value"),
        "string store keeps semantic dispatch"
    );
}
#[test]
fn value_numeric_and_property_consumers_share_checked_element_reads() {
    let call = |arg| {
        Stmt::Expr(Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::LocalGet(F)),
            args: vec![arg],
            type_args: vec![],
            byte_offset: 0,
        })
    };
    let binding = || Stmt::Let {
        id: 10,
        name: "o".into(),
        ty: Type::Any,
        mutable: false,
        init: Some(masked(A)),
    };
    check(&probe_ir(
        "rarr_value_only",
        Type::Array(Box::new(Type::Any)),
        vec![binding(), call(Expr::LocalGet(10))],
        None,
    ));
    check(&probe_ir(
        "rarr_value_numeric",
        number_array(),
        vec![call(add(masked(A), Expr::Number(1.0)))],
        None,
    ));
    check(&probe_ir(
        "rarr_value_no_call",
        Type::Array(Box::new(Type::Any)),
        vec![
            binding(),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(10)),
                property: "d".into(),
                value: Box::new(Expr::LocalGet(I)),
                byte_offset: 0,
                byte_end: 0,
            }),
        ],
        None,
    ));
}
#[test]
fn const_and_mutable_module_arrays_keep_global_roots_across_allocating_loops() {
    let base = vec![
        Stmt::Let {
            id: 10,
            name: "o".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(at(A)),
        },
        Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(10)),
                right: Box::new(Expr::Undefined),
            },
            then_branch: vec![Stmt::Return(Some(Expr::Integer(1)))],
            else_branch: None,
        },
    ];
    for (mutable, allocating) in [(false, false), (false, true), (true, false)] {
        let mut body = base.clone();
        if allocating {
            body.push(Stmt::Expr(Expr::Array(vec![], None)));
        }
        let ty = Type::Array(Box::new(Type::Any));
        let mut m = probe_module("rarr_module_elements", ty.clone(), body, None);
        m.functions[0].params.retain(|p| p.id != A);
        m.init = vec![Stmt::Let {
            id: A,
            name: "a".into(),
            ty,
            mutable,
            init: Some(Expr::Array(vec![], None)),
        }];
        let ir = String::from_utf8(compile_module(&m, opts()).unwrap()).unwrap();
        check(&ir);
        assert!(
            ir.contains("@perry_global_"),
            "module receiver remains a mutable GC root"
        );
        if allocating {
            assert!(
                ir.contains("js_gc_loop") || ir.contains("statepoint"),
                "allocation loops retain moving GC safepoints"
            );
        }
    }
}
