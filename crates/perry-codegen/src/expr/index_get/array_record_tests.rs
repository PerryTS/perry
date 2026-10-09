//! The record's unsigned proof uses the ordinary array read backend.
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Param, Stmt};

fn runtime(method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: "__perry_runtime".into(),
        class_name: None,
        object: None,
        method: method.into(),
        args,
    }
}
fn ir(expr: Expr) -> String {
    ir_body(vec![Stmt::Return(Some(expr))])
}
fn ir_body(body: Vec<Stmt>) -> String {
    let mut m = Module::new("array_record_unsigned");
    m.functions.push(Function {
        id: 1,
        name: "read".into(),
        type_params: vec![],
        params: vec![
            Param {
                id: 1,
                name: "array".into(),
                ty: Type::Array(Box::new(Type::Any)),
                default: None,
                decorators: vec![],
                is_rest: false,
                arguments_object: None,
            },
            Param {
                id: 2,
                name: "cursor".into(),
                ty: Type::Number,
                default: None,
                decorators: vec![],
                is_rest: false,
                arguments_object: None,
            },
        ],
        return_type: Type::Any,
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
    String::from_utf8(
        compile_module(
            &m,
            CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn array_record_unsigned_index_preserves_number_on_cold_read() {
    let ll = ir(Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(runtime("arrayRecordIndex", vec![Expr::LocalGet(2)])),
    });
    let conversion = ll
        .lines()
        .find(|line| line.contains("fptoui double"))
        .expect("unsigned u32 read proof");
    let original = conversion
        .split("fptoui double ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    let fallback = ll
        .lines()
        .find(|line| line.contains("call double @js_typed_feedback_array_index_get_fallback_boxed"))
        .expect("ordinary boxed fallback");
    assert!(
        fallback.contains(&format!("double {original})")),
        "the fallback must receive the original positive JS index above 2^31: {fallback}"
    );
    assert!(
        ll.contains("array_record_index.guard.range") && ll.contains("array_record_index.fast"),
        "the existing guarded array read must consume the proof"
    );
    #[cfg(feature = "llvm-inprocess")]
    crate::testing::verify_ir(&ll, "array_record_unsigned").unwrap();
}

#[test]
fn array_record_rest_appends_with_internal_data_property_operation() {
    let ll = ir(runtime(
        "iteratorRestAppend",
        vec![Expr::LocalGet(1), Expr::LocalGet(2)],
    ));
    assert!(ll
        .lines()
        .any(|line| line.contains("call i64 @js_array_push_f64(")));
    assert!(
        !ll.lines().any(|line| line.contains("call")
            && (line.contains("@js_array_push_dispatch")
                || line.contains("@js_array_method_push"))),
        "rest must not call the user-visible push method"
    );
    #[cfg(feature = "llvm-inprocess")]
    crate::testing::verify_ir(&ll, "array_record_rest").unwrap();
}

#[test]
fn array_record_cursor_increment_keeps_full_unsigned_range() {
    let ll = ir_body(vec![
        Stmt::Let {
            id: 3,
            name: "__idx_3".into(),
            ty: Type::Number,
            mutable: true,
            init: Some(runtime("arrayRecordIndex", vec![Expr::Integer(0)])),
        },
        Stmt::Expr(Expr::Update {
            id: 3,
            op: perry_hir::UpdateOp::Increment,
            prefix: false,
        }),
        Stmt::Return(Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(runtime("arrayRecordIndex", vec![Expr::LocalGet(3)])),
        })),
    ]);
    assert!(
        ll.lines().any(|line| line.contains("fadd double")),
        "the private cursor must retain values across the signed 2^31 boundary"
    );
    assert!(
        !ll.lines().any(|line| line.contains("sitofp i32")),
        "no signed shadow slot may narrow the cursor"
    );
}

#[test]
fn array_record_length_uses_the_existing_indexed_property_read() {
    let ll = ir(runtime("arrayRecordLength", vec![Expr::LocalGet(1)]));
    let indexed = ir(Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(1)),
        property: "length".into(),
        byte_offset: 0,
    });
    assert_eq!(
        ll, indexed,
        "the record must use precisely the indexed length lowering"
    );
    assert!(
        ll.lines().any(|line| line.contains("uitofp i32")),
        "array length is unsigned across 2^31"
    );
    assert!(!ll
        .lines()
        .any(|line| line.contains("call double @js_native_call_method")
            || line.contains("invoke double @js_native_call_method")));
    #[cfg(feature = "llvm-inprocess")]
    crate::testing::verify_ir(&ll, "array_record_length").unwrap();
}

#[test]
fn array_record_length_comparison_is_native_numeric() {
    let ll = ir(Expr::Compare {
        op: perry_hir::CompareOp::Lt,
        left: Box::new(runtime("arrayRecordIndex", vec![Expr::Integer(0)])),
        right: Box::new(runtime("arrayRecordLength", vec![Expr::LocalGet(1)])),
    });
    assert!(ll.lines().any(|line| line.contains("fcmp olt double")));
    assert!(
        !ll.lines()
            .any(|line| (line.contains("call ") || line.contains("invoke "))
                && line.contains("@js_rel_lt")),
        "no dynamic relational fallback"
    );
    assert!(
        !ll.lines()
            .any(|line| line.contains("call") && line.contains("@js_number_coerce")),
        "a record length is already a Number"
    );
}

#[test]
fn array_record_boolean_condition_stays_boolean_through_control_flow() {
    let ll = ir_body(vec![Stmt::If {
        condition: Expr::Sequence(vec![Expr::Conditional {
            condition: Box::new(runtime("arrayRecordNeedsIterator", vec![Expr::LocalGet(1)])),
            then_expr: Box::new(Expr::Bool(true)),
            else_expr: Box::new(Expr::Bool(false)),
        }]),
        then_branch: vec![Stmt::Return(Some(Expr::Integer(1)))],
        else_branch: Some(vec![Stmt::Return(Some(Expr::Integer(0)))]),
    }]);
    assert!(
        !ll.lines()
            .any(|line| line.contains("call") && line.contains("@js_is_truthy")),
        "record control flow is already a Boolean"
    );
}

#[test]
fn array_record_local_flow_preserves_numeric_cursor_and_boolean_entry() {
    let ll = ir_body(vec![
        Stmt::Let {
            id: 3,
            name: "__idx_3".into(),
            ty: Type::Number,
            mutable: true,
            init: Some(runtime("arrayRecordIndex", vec![Expr::Integer(0)])),
        },
        Stmt::Let {
            id: 4,
            name: "record_mode".into(),
            ty: Type::Boolean,
            mutable: false,
            init: Some(runtime("arrayRecordNeedsIterator", vec![Expr::LocalGet(1)])),
        },
        Stmt::For {
            init: None,
            condition: Some(Expr::Compare {
                op: perry_hir::CompareOp::Lt,
                left: Box::new(Expr::LocalGet(3)),
                right: Box::new(runtime("arrayRecordLength", vec![Expr::LocalGet(1)])),
            }),
            update: Some(Expr::Update {
                id: 3,
                op: perry_hir::UpdateOp::Increment,
                prefix: false,
            }),
            body: vec![Stmt::If {
                condition: Expr::Compare {
                    op: perry_hir::CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(4)),
                    right: Box::new(Expr::Bool(true)),
                },
                then_branch: vec![Stmt::Return(Some(Expr::LocalGet(3)))],
                else_branch: None,
            }],
        },
        Stmt::Return(Some(Expr::LocalGet(3))),
    ]);
    for callee in [
        "js_numeric_step",
        "js_to_numeric",
        "js_rel_lt",
        "js_is_truthy",
    ] {
        assert!(
            !ll.lines()
                .any(|line| (line.contains("call ") || line.contains("invoke "))
                    && line.contains(&format!("@{callee}("))),
            "the record primitive contract must survive locals: {callee}"
        );
    }
    assert!(ll.lines().any(|line| line.contains("fadd double")));
    assert!(
        !ll.lines().any(|line| line.contains("sitofp i32")),
        "the cursor must retain the full u32 range"
    );
}

#[test]
fn array_record_index_repairs_the_existing_source_root_after_forwarding() {
    let ll = ir(Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(runtime("arrayRecordIndex", vec![Expr::LocalGet(2)])),
    });
    let label = ll
        .lines()
        .find(|line| line.starts_with("array_record_index.guard.repair") && line.contains(':'))
        .expect("the verified forward edge must repair the source root");
    let block = ll
        .split_once(label)
        .unwrap()
        .1
        .split("\n\n")
        .next()
        .unwrap();
    assert!(
        block.contains("store double") || block.contains("store ptr addrspace(1)"),
        "publish the live head: {block}"
    );
    assert!(block.contains("br label %array_record_index.guard.range"));
    assert!(
        !block.contains("call "),
        "repair needs no helper or allocation"
    );
    #[cfg(feature = "llvm-inprocess")]
    crate::testing::verify_ir(&ll, "array_record_source_repair").unwrap();
}

#[test]
fn ordinary_index_read_preserves_its_existing_forwarding_custody() {
    let ll = ir(Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::LocalGet(2)),
    });
    assert!(
        ll.contains("guard.live"),
        "ordinary read still validates forwarding"
    );
    assert!(
        !ll.contains("guard.repair"),
        "only the private record source needs eager repair"
    );
    #[cfg(feature = "llvm-inprocess")]
    crate::testing::verify_ir(&ll, "ordinary_index_forwarding_custody").unwrap();
}

#[test]
fn completion_flags_preserve_truthiness_without_boolean_constructor_evidence() {
    for (done, constructed) in [
        (Expr::Bool(true), true),
        (
            Expr::Compare {
                op: perry_hir::CompareOp::Eq,
                left: Box::new(Expr::LocalGet(1)),
                right: Box::new(Expr::Bool(true)),
            },
            true,
        ),
        (Expr::LocalGet(1), false),
    ] {
        let ll = ir(runtime(
            "arrayRecordFinish",
            vec![
                Expr::Bool(false),
                Expr::LocalGet(1),
                Expr::Number(0.0),
                Expr::Undefined,
                done,
                Expr::Undefined,
                Expr::Bool(false),
            ],
        ));
        assert!(ll.contains("call double @js_array_record_finish("));
        assert_eq!(
            ll.contains("call i32 @js_is_truthy("),
            !constructed,
            "a declaration must not replace IteratorClose's ToBoolean contract: {ll}"
        );
        #[cfg(feature = "llvm-inprocess")]
        crate::testing::verify_ir(&ll, "record_completion_flags").unwrap();
    }
}

#[test]
fn constructed_done_predicate_keeps_normal_completion_outlined() {
    let done = Expr::Compare {
        op: perry_hir::CompareOp::Eq,
        left: Box::new(Expr::LocalGet(2)),
        right: Box::new(Expr::Bool(true)),
    };
    let ll = ir(Expr::Conditional {
        condition: Box::new(Expr::Compare {
            op: perry_hir::CompareOp::Eq,
            left: Box::new(Expr::LocalGet(2)),
            right: Box::new(Expr::Bool(true)),
        }),
        then_expr: Box::new(runtime(
            "iteratorCloseIfNotDone",
            vec![Expr::LocalGet(1), done.clone()],
        )),
        else_expr: Box::new(runtime(
            "arrayRecordClose",
            vec![
                Expr::LocalGet(1),
                Expr::Number(0.0),
                done,
                Expr::Undefined,
                Expr::Bool(false),
            ],
        )),
    });
    // Numeric parameters also emit a specialized ABI; check each actual
    // consumer body rather than conflating the two generated functions.
    let bodies = ll
        .split("\ndefine ")
        .filter(|body| body.starts_with("internal double @perry_fn_array_record_unsigned__read$"))
        .collect::<Vec<_>>();
    assert!(!bodies.is_empty(), "{ll}");
    for body in bodies {
        assert_eq!(
            body.matches("call double @js_array_record_finish(").count(),
            1,
            "{ll}"
        );
        assert!(!body.contains("call i32 @js_is_truthy("), "{body}");
        assert!(
            !body.contains("call double @js_array_record_close("),
            "{body}"
        );
    }
    #[cfg(feature = "llvm-inprocess")]
    crate::testing::verify_ir(&ll, "constructed_done_completion").unwrap();
}

#[test]
fn ordinary_indexed_length_keeps_main_hot_guard() {
    crate::temp_root_coverage::under_both_lowerings(|_mode| {
        let ll = ir(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: "length".into(),
            byte_offset: 0,
        });
        let check = ll
            .split("plen.check_gc")
            .skip(2)
            .next()
            .expect("guard block");
        let check = check.split("plen.typed_array").next().unwrap();
        assert!(
            check.contains("load i8"),
            "ordinary guard keeps separate header bytes: {check}"
        );
        assert!(
            !check.contains("load i16"),
            "a packed guard changes indexed-loop register allocation"
        );
        assert!(ll.contains("icmp ugt i64") && ll.contains("1048575"));
        assert!(ll.contains("call double @perry_length_cold_"));
        #[cfg(feature = "llvm-inprocess")]
        crate::testing::verify_ir(&ll, "ordinary_length_hot").unwrap();
    });
}

#[test]
fn array_stack_record_uses_one_range_and_one_cold_dispatch() {
    let mode = || Expr::Compare {
        op: perry_hir::CompareOp::Eq,
        left: Box::new(Expr::LocalGet(4)),
        right: Box::new(Expr::Bool(true)),
    };
    let local = |id, ty, init| Stmt::Let {
        id,
        name: format!("record_{id}"),
        ty,
        mutable: true,
        init: Some(init),
    };
    let ll = ir_body(vec![
        local(3, Type::Any, Expr::LocalGet(1)),
        local(
            4,
            Type::Boolean,
            runtime(
                "arrayRecordNeedsIterator",
                vec![Expr::LocalSet(3, Box::new(Expr::LocalGet(3)))],
            ),
        ),
        local(
            5,
            Type::Number,
            runtime("arrayRecordIndex", vec![Expr::Integer(0)]),
        ),
        local(
            6,
            Type::Any,
            runtime(
                "arrayRecordPayload",
                vec![
                    Expr::LocalGet(3),
                    Expr::LocalSet(
                        6,
                        Box::new(Expr::Conditional {
                            condition: Box::new(mode()),
                            then_expr: Box::new(Expr::GetIterator(Box::new(Expr::LocalGet(3)))),
                            else_expr: Box::new(Expr::LocalGet(3)),
                        }),
                    ),
                    Expr::LocalSet(3, Box::new(Expr::Undefined)),
                ],
            ),
        ),
        local(
            7,
            Type::Any,
            runtime("iteratorNextMethod", vec![Expr::LocalGet(6)]),
        ),
        local(8, Type::Any, Expr::Undefined),
        local(9, Type::Number, Expr::Number(2.0)),
        Stmt::For {
            init: None,
            condition: Some(Expr::Compare {
                op: perry_hir::CompareOp::Lt,
                left: Box::new(Expr::LocalGet(5)),
                right: Box::new(runtime(
                    "arrayRecordForBound",
                    vec![
                        mode(),
                        runtime("arrayRecordLength", vec![Expr::LocalGet(6)]),
                        Expr::LocalGet(5),
                        runtime(
                            "iteratorStep",
                            vec![
                                Expr::LocalGet(6),
                                Expr::LocalGet(7),
                                Expr::LocalSet(8, Box::new(Expr::Undefined)),
                                Expr::Bool(true),
                            ],
                        ),
                        Expr::LocalSet(9, Box::new(Expr::Number(2.0))),
                    ],
                )),
            }),
            update: Some(Expr::Update {
                id: 5,
                op: perry_hir::UpdateOp::Increment,
                prefix: false,
            }),
            body: vec![
                Stmt::Expr(Expr::LocalSet(
                    8,
                    Box::new(runtime(
                        "arrayRecordForValue",
                        vec![
                            mode(),
                            Expr::LocalGet(8),
                            Expr::IndexGet {
                                object: Box::new(Expr::LocalGet(6)),
                                index: Box::new(runtime(
                                    "arrayRecordIndex",
                                    vec![Expr::LocalGet(5)],
                                )),
                            },
                        ],
                    )),
                )),
                Stmt::Expr(Expr::LocalSet(9, Box::new(Expr::Number(1.0)))),
            ],
        },
        Stmt::Expr(runtime(
            "arrayRecordFinish",
            vec![
                Expr::LocalGet(4),
                Expr::LocalGet(6),
                Expr::LocalGet(5),
                Expr::LocalGet(6),
                Expr::Bool(true),
                Expr::Undefined,
                Expr::Bool(false),
            ],
        )),
        Stmt::Expr(Expr::LocalSet(7, Box::new(Expr::Undefined))),
        Stmt::Return(Some(Expr::LocalGet(5))),
    ]);
    for field in ll
        .lines()
        .filter(|l| l.contains("getelementptr double, ptr ") && l.ends_with("i64 1"))
    {
        let slot = field.trim().split(" = ").next().unwrap();
        assert!(!ll.lines().any(|l| l.contains("store volatile double") && l.ends_with(&format!("ptr {slot}"))),
            "the shared dispatcher owns next release; a proven loop has no next spill");
    }
    assert!(
        ll.contains("alloca [6 x ptr addrspace(1)]"),
        "one whole native GC home owns the record"
    );
    let calls = ll
        .lines()
        .filter(|l| l.contains("call") && l.contains("@js_array_record_stack_dispatch"))
        .count();
    let consumers = ll
        .split("\ndefine ")
        .filter(|f| {
            f.starts_with("internal double @perry_fn")
                && f.lines().next().unwrap().contains("__read$")
        })
        .count();
    assert_eq!(
        calls,
        consumers * 3,
        "capture, step and finish use the same ABI in each specialization"
    );
    for callee in [
        "js_get_iterator",
        "js_iterator_next_method",
        "js_iterator_step",
        "js_array_record_finish",
    ] {
        assert!(
            !ll.lines()
                .any(|l| l.contains("call") && l.contains(&format!("@{callee}("))),
            "the inline consumer must not duplicate cold protocol: {callee}"
        );
    }
    assert!(
        ll.contains("load volatile double") && ll.contains("store volatile double"),
        "a safepoint cannot cache a record field across a collection"
    );
    crate::testing::verify_ir(&ll, "array_stack_record").unwrap();
}
