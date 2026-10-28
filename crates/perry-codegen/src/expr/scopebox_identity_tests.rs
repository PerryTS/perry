use crate::temp_root_coverage::main_ir_for;
use perry_hir::types::Type;
use perry_hir::{CompareOp, Expr, Stmt};

fn comparison_ir(init: Expr) -> String {
    main_ir_for(
        "scopebox_identity",
        vec![
            Stmt::Let {
                id: 1,
                name: "fixed".into(),
                ty: Type::Object(Default::default()),
                mutable: false,
                init: Some(init),
            },
            Stmt::Let {
                id: 2,
                name: "other".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::PropertyGet {
                    byte_end: 0,
                    object: Box::new(Expr::Object(vec![])),
                    property: "value".into(),
                    byte_offset: 0,
                }),
            },
            Stmt::Expr(Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(1)),
                right: Box::new(Expr::LocalGet(2)),
            }),
        ],
    )
}

#[test]
fn structural_object_identity_uses_tags_without_header_probes() {
    let ir = comparison_ir(Expr::Object(vec![]));
    assert!(
        ir.contains("anyeq.nonobject"),
        "structural allocation proof must select identity: {ir}"
    );
    assert!(
        ir.contains("call i64 @js_eq("),
        "non-pointer values retain exact equality semantics"
    );
    assert!(
        !ir.contains("call i32 @js_jsvalue_equals("),
        "no new identity runtime helper"
    );
}

#[test]
fn an_erased_object_annotation_does_not_prove_identity() {
    let ir = comparison_ir(Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::Object(vec![])),
        property: "value".into(),
        byte_offset: 0,
    });
    assert!(
        !ir.contains("anyeq.nonobject"),
        "a type claim must not skip forwarding: {ir}"
    );
}

#[test]
fn readonly_array_claim_selects_a_runtime_guarded_indexed_read() {
    let ir = main_ir_for(
        "scopebox_readonly",
        vec![
            Stmt::Let {
                id: 1,
                name: "items".into(),
                ty: Type::Generic {
                    base: "ReadonlyArray".into(),
                    type_args: vec![Type::Any],
                },
                mutable: false,
                init: Some(Expr::PropertyGet {
                    byte_end: 0,
                    object: Box::new(Expr::Object(vec![])),
                    property: "value".into(),
                    byte_offset: 0,
                }),
            },
            Stmt::Expr(Expr::IndexGet {
                byte_offset: 0,
                byte_end: 0,
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(0)),
            }),
        ],
    );
    assert!(
        ir.contains("arr.guard.deref"),
        "claim must select a guarded read: {ir}"
    );
}

#[test]
fn a_local_hole_fill_checks_capacity_length_and_prototype() {
    let ir = main_ir_for(
        "scopebox_hole_fill",
        vec![
            Stmt::Let {
                id: 1,
                name: "items".into(),
                ty: Type::Array(Box::new(Type::Any)),
                mutable: true,
                init: Some(Expr::PropertyGet {
                    byte_end: 0,
                    object: Box::new(Expr::Object(vec![])),
                    property: "value".into(),
                    byte_offset: 0,
                }),
            },
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(0)),
                value: Box::new(Expr::Integer(7)),
            }),
        ],
    );
    assert!(
        ir.contains("idxset.hole"),
        "hole fill must select the local guard: {ir}"
    );
    assert!(ir.contains("load volatile i8, ptr @PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED"));
    assert!(
        ir.contains("icmp ult i32"),
        "capacity and length use unsigned bounds"
    );
}

#[test]
fn a_factory_parameter_name_cannot_supply_allocation_provenance() {
    let factory = Expr::Closure {
        func_id: 100,
        params: vec![perry_hir::Param {
            id: 50,
            name: "__perry_obj_iife".into(),
            ty: Type::Any,
            default: None,
            decorators: vec![],
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::Array(vec![], None)))],
        captures: vec![],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    };
    let ir = comparison_ir(Expr::Call {
        byte_end: 0,
        callee: Box::new(factory),
        args: vec![Expr::Object(vec![])],
        type_args: vec![],
        byte_offset: 0,
    });
    assert!(
        !ir.contains("anyeq.nonobject"),
        "a factory may return an array: {ir}"
    );
}
