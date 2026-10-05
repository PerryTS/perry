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
    let signature = output
        .lines()
        .find(|l| {
            l.starts_with("define internal ")
                && l.contains("@perry_fn_length_region__probe")
                && !l.contains("$generic(")
        })
        .unwrap_or_else(|| {
            panic!(
                "no numeric clone: {}",
                output
                    .lines()
                    .filter(|l| l.starts_with("define "))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        });
    let at = output.find(signature).unwrap();
    let f = &output[at..];
    f[..f.find("\n}\n").expect("function end")].to_string()
}

#[test]
fn length_bound_guarded_copy_has_no_per_access_check() {
    for (op, offset, other) in [
        (CompareOp::Lt, Expr::Integer(0), false),
        (CompareOp::Le, Expr::Integer(1), false),
        (CompareOp::Lt, Expr::Integer(2), false),
        (CompareOp::Lt, Expr::LocalGet(7), false),
        (CompareOp::Lt, Expr::Integer(0), true),
    ] {
        let ir = ir(op, offset, other, 0, false);
        assert!(
            ir.contains("rloop.fast"),
            "no region for {op:?}, other={other}:\n{ir}"
        );
        let fast_label = ir
            .lines()
            .find(|l| l.starts_with("rloop.fast.") && l.ends_with(':'))
            .unwrap();
        let fast = &ir[ir.find(fast_label).unwrap() + fast_label.len()..];
        let fast = fast.split("\n\n").next().unwrap();
        assert!(fast.matches("load double, ptr").count() >= 2, "{fast}");
        assert!(
            !fast.contains("icmp ult")
                && !fast.contains("@llvm.assume")
                && !fast.contains("pview.get.oob")
                && !fast.contains("pview.gget"),
            "{fast}"
        );
        // Witness: the plain copy still tests its accesses (either a checked
        // branch or the existing sealed-length assume proof).
        assert!(
            ir.contains("icmp ult i32") || ir.contains("pview.gget.slow"),
            "{ir}"
        );
        let guard = ir.split("rloop.version.split").next().unwrap();
        assert!(
            guard
                .lines()
                .any(|l| l.contains("fcmp ole double") && !l.contains("2147483647.0")),
            "missing end <= length: {ir}"
        );
        // The guard has plain header reads. Sealed loop conditions may carry
        // invariant metadata elsewhere, but the guard must not.
        let start = guard.rfind("getelementptr i8").unwrap();
        let tail = &guard[start..];
        assert!(
            tail.contains("load i32, ptr") && !tail.contains("!invariant.load"),
            "{tail}"
        );
    }
}

#[test]
fn observing_buffer_refuses_length_region() {
    let ir = ir(CompareOp::Lt, Expr::Integer(0), false, 1, false);
    assert!(!ir.contains("rloop.fast"), "{ir}");
    assert!(!ir.contains("!invariant.load"), "{ir}");
}

#[test]
fn length_range_offsets_preserve_source_and_reject_negative_indices() {
    let mut env = Env::default();
    env.ranges.insert(
        5,
        Rng::checked(Some(0), Some(Hi::Lt(Symbol::ViewLength(3, None), -2)), true).unwrap(),
    );
    let end = env
        .view_end(&bin(BinaryOp::Add, Expr::LocalGet(5), Expr::Integer(2)))
        .unwrap();
    assert_eq!(end.end, End::Sym(Symbol::ViewLength(3, None), 0));
    assert!(end.counter);
    assert!(env
        .view_end(&bin(BinaryOp::Sub, Expr::LocalGet(5), Expr::Integer(1)))
        .is_none());
    let u = ArrayUse {
        view: true,
        view_counter: true,
        view_sym: Some((Symbol::ViewLength(3, None), 0)),
        ..ArrayUse::default()
    };
    assert!(u.view_covers(end));
    assert!(!u.view_covers(ViewEnd {
        end: End::Sym(Symbol::ViewLength(4, None), 0),
        counter: true
    }));
}

#[test]
fn late_exposed_length_is_plain_at_entry_and_every_recheck() {
    let ir = ir(CompareOp::Lt, Expr::Integer(0), false, 2, true);
    assert!(
        ir.contains("rloop.fast") && ir.contains("rloop.recheck"),
        "{ir}"
    );
    assert!(!ir.contains("!invariant.load"), "{ir}");
    let label = ir
        .lines()
        .find(|l| l.starts_with("rloop.recheck.") && l.ends_with(':'))
        .unwrap();
    let rc = &ir[ir.find(label).unwrap() + label.len()..];
    let rc = rc.split("\n\n").next().unwrap();
    assert!(
        rc.matches("load i32, ptr").count() >= 2 && rc.contains("fcmp ole double"),
        "{rc}"
    );
    assert!(!rc.contains("!invariant.load"), "{rc}");
}

#[test]
fn symbolic_length_guard_contains_local_subtraction() {
    let ir = ir(CompareOp::Lt, Expr::LocalGet(7), false, 0, false);
    let guard = ir.split("rloop.version.split").next().unwrap();
    let start = guard.rfind("fcmp oge double").expect("counter entry guard");
    let guard = &guard[start..];
    assert!(guard.contains("fsub double"), "{guard}");
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
        .find(|l| l.starts_with("for.cond.") && l.ends_with(':'))
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
fn counter_entry_guard_checks_nonnegative_i32_and_integrality() {
    let ir = ir(CompareOp::Lt, Expr::Integer(0), false, 0, false);
    let guard = ir.split("rloop.version.split").next().unwrap();
    assert!(
        guard.contains("fcmp oge double") && guard.contains("2147483647.0"),
        "{guard}"
    );
    assert!(guard.contains("fcmp oeq double"), "{guard}");
    assert!(
        guard.contains("select i1") && guard.contains("fptosi double"),
        "{guard}"
    );
}

#[test]
fn inclusive_length_minus_one_has_exclusive_end_at_length() {
    let ir = ir(CompareOp::Le, Expr::Integer(1), false, 0, false);
    let guard = ir.split("rloop.version.split").next().unwrap();
    let start = guard.rfind("fcmp oge double").unwrap();
    assert!(
        guard[start..]
            .lines()
            .any(|l| l.contains("fadd double") && l.ends_with(", 0.0")),
        "{guard}"
    );
}

#[test]
fn symbolic_length_reads_the_unsigned_header_value() {
    let ir = ir(CompareOp::Lt, Expr::Integer(0), false, 0, false);
    let guard = ir.split("rloop.version.split").next().unwrap();
    let start = guard.rfind("fcmp oge double").unwrap();
    assert!(guard[start..].contains("uitofp i32"), "{guard}");
}

#[test]
fn a_recheck_requires_the_counter_below_the_live_exclusive_bound() {
    // After JS shrinks both source and target to zero, end <= length passes.
    // The iteration's earlier condition does not justify a remaining access.
    let ir = ir(CompareOp::Le, Expr::Integer(1), false, 2, true);
    let label = ir
        .lines()
        .find(|l| l.starts_with("rloop.recheck.") && l.ends_with(':'))
        .expect("a recheck");
    let rc = &ir[ir.find(label).unwrap() + label.len()..];
    let rc = rc.split("\n\n").next().unwrap();
    assert!(rc.contains("fcmp olt double"), "{rc}");
    assert!(rc.contains("fcmp ole double"), "{rc}");
}
