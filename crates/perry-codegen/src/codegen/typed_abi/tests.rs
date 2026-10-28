use super::*;
use perry_hir::{Class, Param};

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

fn function(return_type: Type, params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id: 1,
        name: "mixed".to_string(),
        type_params: Vec::new(),
        params,
        return_type,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn ret(expr: Expr) -> Vec<Stmt> {
    vec![Stmt::Return(Some(expr))]
}

fn class(id: u32, name: &str, extends: Option<u32>, extends_name: Option<&str>) -> Class {
    Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends,
        extends_name: extends_name.map(str::to_string),
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

#[test]
fn receiver_clone_rejects_name_only_imported_parent_chain() {
    let parent = class(1, "Parent", None, None);
    let child = class(2, "Child", None, Some("Parent"));
    let classes = HashMap::from([("Parent".to_string(), &parent)]);

    assert!(matches!(
        typed_receiver_chain_fields(&classes, &child, false),
        Err(TypedCloneRejectionReason::ReceiverClassExtends)
    ));
}

#[test]
fn f64_clone_accepts_mixed_raw_params_when_return_expr_is_numeric_safe() {
    let f = function(
        Type::Number,
        vec![
            param(10, "n", Type::Number),
            param(11, "i", Type::Int32),
            param(12, "flag", Type::Boolean),
        ],
        ret(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(10)),
            right: Box::new(Expr::LocalGet(11)),
        }),
    );

    assert_eq!(typed_f64_function_rejection_reason(&f), None);
    assert_eq!(
        typed_param_reps_for_params(&f.params),
        Some(vec![
            TypedParamRep::F64,
            TypedParamRep::I32,
            TypedParamRep::I1
        ])
    );
}

#[test]
fn f64_clone_accepts_raw_i32_locals_before_numeric_return() {
    let f = function(
        Type::Number,
        vec![param(10, "n", Type::Number), param(11, "i", Type::Int32)],
        vec![
            Stmt::Let {
                id: 12,
                name: "mask".to_string(),
                ty: Type::Int32,
                mutable: false,
                init: Some(Expr::Binary {
                    op: BinaryOp::BitOr,
                    left: Box::new(Expr::LocalGet(11)),
                    right: Box::new(Expr::Integer(1)),
                }),
            },
            Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(10)),
                right: Box::new(Expr::LocalGet(12)),
            })),
        ],
    );

    assert_eq!(typed_f64_function_rejection_reason(&f), None);
}

#[test]
fn f64_clone_rejects_unsafe_mixed_rep_use() {
    let f = function(
        Type::Number,
        vec![
            param(10, "n", Type::Number),
            param(11, "flag", Type::Boolean),
        ],
        ret(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(10)),
            right: Box::new(Expr::LocalGet(11)),
        }),
    );

    assert_eq!(
        typed_f64_function_rejection_reason(&f),
        Some(TypedCloneRejectionReason::ReturnExprNotTypedF64Safe)
    );
}

#[test]
fn string_clone_accepts_mixed_params_when_only_string_rep_flows_to_return() {
    let f = function(
        Type::String,
        vec![
            param(10, "s", Type::String),
            param(11, "i", Type::Int32),
            param(12, "flag", Type::Boolean),
        ],
        ret(Expr::LocalGet(10)),
    );

    assert_eq!(typed_string_function_rejection_reason(&f), None);
}

#[test]
fn closure_capture_representations_are_runtime_guard_candidates() {
    let expr = Expr::Closure {
        func_id: 7,
        params: vec![param(20, "scale", Type::Number)],
        return_type: Type::Number,
        body: ret(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(20)),
            right: Box::new(Expr::LocalGet(30)),
        }),
        captures: vec![30, 31],
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    };
    let module_local_types = HashMap::from([(30, Type::Int32), (31, Type::Boolean)]);

    assert_eq!(
        typed_f64_closure_rejection_reason_with_types(&expr, &module_local_types),
        None
    );
    assert_eq!(
        typed_f64_closure_capture_reps(&expr, &module_local_types),
        Some(vec![(30, TypedParamRep::I32), (31, TypedParamRep::I1)])
    );
}

#[test]
fn closure_capture_representations_follow_effective_slot_order() {
    let expr = Expr::Closure {
        func_id: 8,
        params: vec![param(20, "scale", Type::Number)],
        return_type: Type::Number,
        body: ret(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(20)),
            // Deliberately absent from the explicit capture list: closure
            // allocation and body lowering append this auto-capture.
            right: Box::new(Expr::LocalGet(29)),
        }),
        captures: vec![30, 31],
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    };
    let module_local_types =
        HashMap::from([(29, Type::Number), (30, Type::Int32), (31, Type::Boolean)]);

    assert_eq!(
        typed_f64_closure_capture_reps(&expr, &module_local_types),
        Some(vec![
            (30, TypedParamRep::I32),
            (31, TypedParamRep::I1),
            (29, TypedParamRep::F64),
        ])
    );
}

#[test]
fn typed_number_remainder_keeps_integer_check_and_ieee_fallback() {
    use crate::block::{LlBlock, RegCounter};
    use std::rc::Rc;

    let params = vec![param(10, "a", Type::Number)];
    let mut block = LlBlock::new("entry", Rc::new(RegCounter::new()));
    // The same lowering serves free functions and captured closure bodies.
    // A raw numeric capture is admitted by its value guard, not its type.
    let body = ret(Expr::Binary {
        op: BinaryOp::Mod,
        left: Box::new(Expr::LocalGet(10)),
        right: Box::new(Expr::LocalGet(20)),
    });
    lower_typed_f64_body_with_seed_locals_and_reps(
        &mut block,
        &params,
        &body,
        HashMap::from([(20, "%capture".to_string())]),
        HashMap::from([(20, TypedParamRep::F64)]),
    )
    .unwrap();
    let ir = block.to_ir();
    assert!(
        ir.contains("call double @perry_checked_number_remainder(double %arg10, double %capture)"),
        "{ir}"
    );
    assert!(
        !ir.contains("frem"),
        "typed Number remainder must retain the exact integer path: {ir}"
    );

    let receiver = TypedReceiverMethodInfo {
        fields: vec![TypedReceiverField {
            name: "divisor".to_string(),
            index: 0,
        }],
    };
    let body = ret(Expr::Binary {
        op: BinaryOp::Mod,
        left: Box::new(Expr::LocalGet(10)),
        right: Box::new(Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(Expr::This),
            property: "divisor".to_string(),
            byte_offset: 0,
        }),
    });
    let mut block = LlBlock::new("entry", Rc::new(RegCounter::new()));
    lower_typed_f64_receiver_body(&mut block, &params, &body, &receiver, 24).unwrap();
    let ir = block.to_ir();
    assert!(
        ir.contains("call double @perry_checked_number_remainder("),
        "{ir}"
    );
    assert!(!ir.contains("frem"), "{ir}");
}

#[test]
fn numeric_captures_share_the_actual_value_ordered_entry_tier() {
    let mut module = perry_hir::Module::new("numeric_capture_entry");
    let closure = Expr::Closure {
        func_id: 2,
        params: vec![param(30, "x", Type::Number)],
        return_type: Type::Number,
        body: ret(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(30)),
            right: Box::new(Expr::LocalGet(20)),
        }),
        captures: vec![20],
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    };
    module.functions.push(function(
        Type::Any,
        vec![param(20, "capture", Type::Number)],
        ret(closure),
    ));
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let ir = String::from_utf8(crate::compile_module(&module, opts).unwrap()).unwrap();
    let public = ir
        .split("define ")
        .find(|f| {
            f.lines()
                .next()
                .is_some_and(|line| line.contains("@perry_closure_numeric_capture_entry__2("))
        })
        .expect("public closure entry");
    let entry = public.split("typed_closure_public.fast").next().unwrap();
    assert_eq!(entry.matches("fcmp ord double").count(), 2, "{public}");
    assert!(public.contains("typed_closure_public.int32"), "{public}");
    assert!(public.contains("$generic"), "{public}");
}
