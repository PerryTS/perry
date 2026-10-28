//! Length/counter semantic inputs ported from removed array-region proofs.
use super::*;
use perry_hir::{types::Type, Function, Module, Param, UpdateOp};

fn bin(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}
fn local(id: u32, mutable: bool, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: if matches!(&init, Expr::TypedArrayNew { .. }) {
            Type::Named("Float64Array".into())
        } else {
            Type::Number
        },
        mutable,
        init: Some(init),
    }
}
fn len(id: u32) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(id)),
        property: "length".into(),
        byte_offset: 0,
    }
}
fn get(id: u32, index: Expr) -> Expr {
    Expr::IndexGet {
        object: Box::new(Expr::LocalGet(id)),
        index: Box::new(index),
    }
}

fn ir(op: CompareOp, offset: Expr, other: bool, exposure: u8, recheck: bool) -> String {
    let mut body = vec![
        local(
            2,
            false,
            bin(
                BinaryOp::Add,
                bin(BinaryOp::BitAnd, Expr::LocalGet(1), Expr::Integer(15)),
                Expr::Integer(8),
            ),
        ),
        local(
            3,
            false,
            Expr::TypedArrayNew {
                kind: perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                arg: Some(Box::new(Expr::LocalGet(2))),
            },
        ),
        local(
            4,
            false,
            Expr::TypedArrayNew {
                kind: perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                arg: Some(Box::new(bin(
                    BinaryOp::Sub,
                    Expr::LocalGet(2),
                    Expr::Integer(1),
                ))),
            },
        ),
        local(7, false, Expr::Integer(2)),
        local(6, true, Expr::Number(0.0)),
    ];
    if exposure == 1 {
        body.push(Stmt::Expr(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(3)),
            property: "buffer".into(),
            byte_offset: 0,
        }));
    }
    let mut loop_body = vec![Stmt::Expr(Expr::LocalSet(
        6,
        Box::new(bin(
            BinaryOp::Add,
            Expr::LocalGet(6),
            bin(
                BinaryOp::Add,
                get(3, bin(BinaryOp::Sub, Expr::LocalGet(5), Expr::Integer(0))),
                bin(
                    BinaryOp::Mul,
                    get(3, bin(BinaryOp::Sub, Expr::LocalGet(5), Expr::Integer(0))),
                    Expr::Number(2.0),
                ),
            ),
        )),
    ))];
    if recheck {
        loop_body.push(Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(5)),
                right: Box::new(Expr::Integer(3)),
            },
            then_branch: vec![Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::FuncRef(2)),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
            })],
            else_branch: None,
        });
    }
    body.push(Stmt::For {
        init: Some(Box::new(local(5, true, Expr::Integer(0)))),
        condition: Some(Expr::Compare {
            op,
            left: Box::new(Expr::LocalGet(5)),
            right: Box::new(bin(BinaryOp::Sub, len(if other { 4 } else { 3 }), offset)),
        }),
        update: Some(Expr::Update {
            id: 5,
            op: UpdateOp::Increment,
            prefix: false,
        }),
        body: loop_body,
    });
    if exposure == 2 {
        body.push(Stmt::Expr(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(3)),
            property: "buffer".into(),
            byte_offset: 0,
        }));
    }
    body.push(Stmt::Return(Some(Expr::LocalGet(6))));
    let mut m = Module::new("length_region");
    m.functions.push(Function {
        id: 1,
        name: "probe".into(),
        type_params: vec![],
        params: vec![Param {
            id: 1,
            name: "seed".into(),
            ty: Type::Number,
            default: None,
            decorators: vec![],
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Number,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    if recheck {
        m.functions.push(Function {
            id: 2,
            name: "tick".into(),
            type_params: vec![],
            params: vec![],
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::Integer(0)))],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: vec![],
            decorators: vec![],
            was_plain_async: false,
            was_unrolled: false,
        });
    }
    let output = String::from_utf8(
        crate::compile_module(
            &m,
            crate::CompileOptions {
                emit_ir_only: true,
                is_entry_module: false,
                verify_native_regions: true,
                ..crate::CompileOptions::default()
            },
        )
        .expect("length region compiles"),
    )
    .unwrap();
    output
}

fn check_view(ir: &str) {
    crate::testing::verify_ir(ir, "array_layout_length").expect("valid checked view lowering");
    assert!(
        !ir.contains("rloop.fast") && !ir.contains("rloop.recheck"),
        "there is one checked loop, without runtime array region clones"
    );
    assert!(
        ir.contains("load i32") || ir.contains("js_buffer_length"),
        "length remains an unsigned live receiver fact"
    );
}
#[test]
fn own_other_inclusive_constant_and_symbolic_length_bounds_compile() {
    for (op, offset, other) in [
        (CompareOp::Lt, Expr::Integer(0), false),
        (CompareOp::Le, Expr::Integer(1), false),
        (CompareOp::Lt, Expr::Integer(2), false),
        (CompareOp::Lt, Expr::LocalGet(7), false),
        (CompareOp::Lt, Expr::Integer(0), true),
    ] {
        check_view(&ir(op, offset, other, 0, false));
    }
}
#[test]
fn observing_buffer_keeps_live_length() {
    let ir = ir(CompareOp::Lt, Expr::Integer(0), false, 1, false);
    check_view(&ir);
    assert!(
        !ir.contains("!invariant.load"),
        "exposed storage length cannot be invariant"
    );
}
#[test]
fn negative_and_other_receiver_offsets_use_checked_accesses() {
    for offset in [Expr::Integer(-1), Expr::Integer(1), Expr::Integer(2)] {
        let ir = ir(CompareOp::Lt, offset, true, 0, false);
        check_view(&ir);
        assert!(
            ir.contains("icmp ult") || ir.contains("index_get") || ir.contains("pview.gget"),
            "an offset never lends another receiver an unchecked bound"
        );
    }
}
#[test]
fn late_buffer_exposure_and_call_keep_live_length() {
    let ir = ir(CompareOp::Lt, Expr::Integer(0), false, 2, true);
    check_view(&ir);
    assert!(
        !ir.contains("!invariant.load"),
        "the next iteration observes changed storage"
    );
}
#[test]
fn symbolic_bound_retains_subtraction() {
    let ir = ir(CompareOp::Lt, Expr::LocalGet(7), false, 0, false);
    check_view(&ir);
    assert!(
        ir.contains("fsub double") || ir.contains("sub i32"),
        "source subtraction remains"
    );
}
#[test]
fn a_typed_annotation_does_not_hoist_a_possible_length_getter() {
    let mut m = Module::new("getter_bound");
    m.functions.push(Function {
        id: 1,
        name: "probe".into(),
        type_params: vec![],
        params: vec![Param {
            id: 3,
            name: "a".into(),
            ty: Type::Named("Float64Array".into()),
            default: None,
            decorators: vec![],
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Number,
        body: vec![
            local(6, true, Expr::Number(0.0)),
            Stmt::For {
                init: Some(Box::new(local(5, true, Expr::Integer(0)))),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(5)),
                    right: Box::new(len(3)),
                }),
                update: Some(Expr::Update {
                    id: 5,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    6,
                    Box::new(bin(
                        BinaryOp::Add,
                        Expr::LocalGet(6),
                        bin(
                            BinaryOp::Add,
                            get(3, Expr::LocalGet(5)),
                            get(3, Expr::LocalGet(5)),
                        ),
                    )),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(6))),
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
    let output = String::from_utf8(
        crate::compile_module(
            &m,
            crate::CompileOptions {
                emit_ir_only: true,
                is_entry_module: false,
                ..crate::CompileOptions::default()
            },
        )
        .unwrap(),
    )
    .unwrap();
    let body = crate::testing::root_slots::function_slice(&output, "perry_fn_getter_bound__probe");
    let cond = body
        .lines()
        .find(|l| {
            (l.starts_with("for.cond.") || l.starts_with("for.indexed_owner_slow.cond."))
                && l.ends_with(':')
        })
        .expect("loop condition");
    let before = &body[..body.find(cond).unwrap()];
    assert!(
        !before.contains("@js_value_length_f64(")
            && !before.contains("@js_value_length_property_f64("),
        "{before}"
    );
    let cond_body = &body[body.find(cond).unwrap() + cond.len()..];
    assert!(
        cond_body.contains("length_property") || cond_body.contains("js_value_length"),
        "{cond_body}"
    );
    assert!(!body.contains("rloop.fast"), "{body}");
}
#[test]
fn counter_and_length_are_checked_at_each_access() {
    let ir = ir(CompareOp::Lt, Expr::Integer(0), false, 0, false);
    check_view(&ir);
    assert!(
        ir.contains("icmp ult") || ir.contains("index_get") || ir.contains("pview.gget"),
        "a source counter is not an unchecked array-region proof"
    );
}
#[test]
fn inclusive_length_minus_one_retains_inclusive_comparison() {
    let ir = ir(CompareOp::Le, Expr::Integer(1), false, 0, false);
    check_view(&ir);
    assert!(
        ir.contains("icmp sle") || ir.contains("fcmp ole"),
        "inclusive condition remains"
    );
}
#[test]
fn unsigned_length_conversion_is_retained() {
    let ir = ir(CompareOp::Lt, Expr::Integer(0), false, 0, false);
    check_view(&ir);
    assert!(
        ir.contains("uitofp i32") || ir.contains("zext i32"),
        "a view length must not become negative at bit 31"
    );
}
#[test]
fn shrinking_both_receivers_does_not_keep_a_pre_call_region() {
    let ir = ir(CompareOp::Le, Expr::Integer(1), false, 2, true);
    check_view(&ir);
    assert!(
        ir.contains("icmp ult") || ir.contains("index_get") || ir.contains("pview.gget"),
        "current length controls access after a call"
    );
}
