use super::*;
use perry_hir::infer_expr_type;
use std::collections::HashMap;

#[test]
fn hir_inferred_refinable_type_reuses_codegen_local_types() {
    let mut local_types = HashMap::new();
    local_types.insert(7, HirType::String);

    assert_eq!(
        hir_inferred_refinable_type_from_locals(&local_types, &Expr::LocalGet(7)),
        Some(HirType::String)
    );
}

#[test]
fn hir_inferred_refinable_type_filters_escape_hatch_types() {
    let local_types = HashMap::new();

    assert_eq!(
        hir_inferred_refinable_type_from_locals(&local_types, &Expr::LocalGet(99)),
        None
    );
    assert_eq!(
        hir_inferred_refinable_type_from_locals(&local_types, &Expr::Undefined),
        None
    );
    assert_eq!(
        hir_inferred_refinable_type_from_locals(&local_types, &Expr::ProcessExit(None)),
        None
    );
}

#[test]
fn hir_inferred_refinable_type_keeps_path_to_namespaced_path_conservative() {
    let mut local_types = HashMap::new();
    local_types.insert(1, HirType::String);
    local_types.insert(2, HirType::Number);

    assert_eq!(
        hir_inferred_refinable_type_from_locals(
            &local_types,
            &Expr::PathToNamespacedPath(Box::new(Expr::LocalGet(1))),
        ),
        Some(HirType::String)
    );
    assert_eq!(
        hir_inferred_refinable_type_from_locals(
            &local_types,
            &Expr::PathToNamespacedPath(Box::new(Expr::LocalGet(2))),
        ),
        None
    );
}

#[test]
fn hir_inferred_static_type_provides_codegen_fallback_facts() {
    let mut local_types = HashMap::new();
    local_types.insert(1, HirType::Array(Box::new(HirType::String)));

    assert_eq!(
        hir_inferred_static_type_from_locals(&local_types, &Expr::ProcessArgv),
        Some(HirType::Array(Box::new(HirType::String)))
    );
    assert_eq!(
        hir_inferred_static_type_from_locals(
            &local_types,
            &Expr::New {
                class_name: "Array".to_string(),
                args: vec![Expr::Integer(4)],
                type_args: vec![],
                byte_offset: 0,
                cap_args_appended: 0,
            },
        ),
        Some(HirType::Array(Box::new(HirType::Any)))
    );
    assert_eq!(
        hir_inferred_static_type_from_locals(
            &local_types,
            &Expr::IndexGet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(0)),
            },
        ),
        Some(HirType::String)
    );
    assert_eq!(
        hir_inferred_static_type_from_locals(
            &local_types,
            &Expr::PropertyGet {
                object: Box::new(Expr::Object(vec![(
                    "answer".to_string(),
                    Expr::Number(1.0)
                )])),
                property: "answer".to_string(),
            },
        ),
        Some(HirType::Number)
    );
    assert_eq!(
        hir_inferred_refinable_type_from_locals(
            &local_types,
            &Expr::MapNewFromArray(Box::new(Expr::Array(vec![Expr::Array(vec![
                Expr::String("answer".to_string()),
                Expr::Number(1.0),
            ])]))),
        ),
        Some(HirType::Generic {
            base: "Map".to_string(),
            type_args: vec![HirType::String, HirType::Number]
        })
    );
}

#[test]
fn hir_inferred_types_reuse_imported_function_return_facts() {
    let local_types = HashMap::new();
    let imported_func_return_types = HashMap::from([("readName".to_string(), HirType::String)]);
    let classes = HashMap::new();
    let interfaces = HashMap::new();
    let class_stack = Vec::new();
    let enums = HashMap::new();
    let facts = CodegenTypeFacts {
        local_types: &local_types,
        imported_func_return_types: &imported_func_return_types,
        classes: &classes,
        interfaces: &interfaces,
        class_stack: &class_stack,
        enums: &enums,
    };
    let call = Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "readName".to_string(),
            param_types: vec![],
            return_type: HirType::Any,
        }),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    };

    assert_eq!(
        hir_inferred_refinable_type_from_facts(&facts, &call),
        Some(HirType::String)
    );
    assert_eq!(infer_expr_type(&call, &facts), HirType::String);
}

#[test]
fn hir_inferred_types_reuse_codegen_contextual_class_facts() {
    let mut local_types = HashMap::new();
    local_types.insert(1, HirType::Named("Widget".to_string()));
    let imported_func_return_types = HashMap::new();
    let class_stack = vec!["Widget".to_string()];
    let interfaces = HashMap::new();
    let enums = HashMap::from([(
        ("Mode".to_string(), "Auto".to_string()),
        perry_hir::EnumValue::String("auto".to_string()),
    )]);
    let base = perry_hir::Class {
        id: 2,
        name: "Base".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: vec![perry_hir::Function {
            id: 4,
            name: "baseScore".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: HirType::Number,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }],
        getters: vec![(
            "baseLabel".to_string(),
            perry_hir::Function {
                id: 5,
                name: "get_baseLabel".to_string(),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: HirType::String,
                body: Vec::new(),
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            },
        )],
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        is_nested: false,
        aliases: Vec::new(),
    };
    let widget = perry_hir::Class {
        id: 1,
        name: "Widget".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: Some("Base".to_string()),
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![perry_hir::ClassField {
            name: "label".to_string(),
            key_expr: None,
            ty: HirType::String,
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
        constructor: None,
        methods: vec![perry_hir::Function {
            id: 3,
            name: "score".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: HirType::Number,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }],
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: vec![perry_hir::ClassField {
            name: "count".to_string(),
            key_expr: None,
            ty: HirType::Number,
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
        static_methods: vec![perry_hir::Function {
            id: 2,
            name: "make".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: HirType::Named("Widget".to_string()),
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }],
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        is_nested: false,
        aliases: Vec::new(),
    };
    let classes = HashMap::from([("Base".to_string(), &base), ("Widget".to_string(), &widget)]);
    let facts = CodegenTypeFacts {
        local_types: &local_types,
        imported_func_return_types: &imported_func_return_types,
        classes: &classes,
        interfaces: &interfaces,
        class_stack: &class_stack,
        enums: &enums,
    };

    assert_eq!(
        infer_expr_type(&Expr::This, &facts),
        HirType::Named("Widget".to_string())
    );
    assert_eq!(
        infer_expr_type(
            &Expr::EnumMember {
                enum_name: "Mode".to_string(),
                member_name: "Auto".to_string(),
            },
            &facts,
        ),
        HirType::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "label".to_string(),
            },
            &facts,
        ),
        HirType::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: "score".to_string(),
                }),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            },
            &facts,
        ),
        HirType::Number
    );
    assert_eq!(
        infer_expr_type(
            &Expr::SuperMethodCall {
                method: "baseScore".to_string(),
                args: Vec::new(),
            },
            &facts,
        ),
        HirType::Number
    );
    assert_eq!(
        infer_expr_type(
            &Expr::SuperPropertyGet {
                property: "baseLabel".to_string(),
            },
            &facts,
        ),
        HirType::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::SuperPropertyGet {
                property: "baseScore".to_string(),
            },
            &facts,
        ),
        HirType::Function(perry_types::FunctionType {
            params: Vec::new(),
            return_type: Box::new(HirType::Number),
            is_async: false,
            is_generator: false,
        })
    );
    assert_eq!(
        infer_expr_type(
            &Expr::StaticFieldGet {
                class_name: "Widget".to_string(),
                field_name: "count".to_string(),
            },
            &facts,
        ),
        HirType::Number
    );
    assert_eq!(
        infer_expr_type(
            &Expr::StaticMethodCall {
                class_name: "Widget".to_string(),
                method_name: "make".to_string(),
                args: Vec::new(),
            },
            &facts,
        ),
        HirType::Named("Widget".to_string())
    );
}

#[test]
fn function_return_type_is_conservative() {
    // Documents the deliberate `None`: codegen doesn't thread a local
    // function-return-type map, so direct `FuncRef` calls infer `Any` rather
    // than a declared type. If this is ever wired in, this test should be
    // updated alongside it.
    use perry_hir::HirTypeFacts as _;
    let local_types = HashMap::new();
    let imported_func_return_types = HashMap::new();
    let classes = HashMap::new();
    let interfaces = HashMap::new();
    let class_stack = Vec::new();
    let enums = HashMap::new();
    let facts = CodegenTypeFacts {
        local_types: &local_types,
        imported_func_return_types: &imported_func_return_types,
        classes: &classes,
        interfaces: &interfaces,
        class_stack: &class_stack,
        enums: &enums,
    };
    assert_eq!(facts.function_return_type(0), None);
}

#[test]
fn tuple_index_literal_only_accepts_nonneg_integers() {
    assert_eq!(tuple_index_literal(&Expr::Integer(2)), Some(2));
    assert_eq!(tuple_index_literal(&Expr::Number(1.0)), Some(1));
    // Negative, fractional, or non-literal indices aren't statically known.
    assert_eq!(tuple_index_literal(&Expr::Integer(-1)), None);
    assert_eq!(tuple_index_literal(&Expr::Number(1.5)), None);
    assert_eq!(tuple_index_literal(&Expr::LocalGet(0)), None);
}
