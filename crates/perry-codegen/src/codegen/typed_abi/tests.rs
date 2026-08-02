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
    }
}

#[test]
fn receiver_clone_rejects_name_only_imported_parent_chain() {
    let parent = class(1, "Parent", None, None);
    let child = class(2, "Child", None, Some("Parent"));
    let classes = HashMap::from([("Parent".to_string(), &parent)]);

    assert!(matches!(
        typed_receiver_chain_fields(&classes, &child),
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
fn closure_clone_accepts_mixed_immutable_captures_for_numeric_return() {
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
