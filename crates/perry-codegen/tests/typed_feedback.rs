use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{
    BinaryOp, Class, ClassField, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt,
    UpdateOp,
};
use perry_types::{FunctionType, Type};

fn empty_opts() -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: false,
        non_entry_module_prefixes: Vec::new(),
        import_function_prefixes: std::collections::HashMap::new(),
        import_function_origin_names: std::collections::HashMap::new(),
        import_function_v8_specifiers: std::collections::HashMap::new(),
        import_function_node_submodule: std::collections::HashMap::new(),
        namespace_node_submodules: std::collections::HashMap::new(),
        namespace_v8_specifiers: std::collections::HashMap::new(),
        namespace_member_prefixes: std::collections::HashMap::new(),
        emit_ir_only: true,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        namespace_imports: Vec::new(),
        namespace_reexport_named_imports: std::collections::HashSet::new(),
        imported_classes: Vec::new(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: std::collections::HashMap::new(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_synthetic_arguments: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        needs_stdlib: false,
        needs_ui: false,
        needs_geisterhand: false,
        geisterhand_port: 7676,
        enabled_features: Vec::new(),
        native_module_init_names: Vec::new(),
        js_module_specifiers: Vec::new(),
        bundled_extensions: Vec::new(),
        native_library_functions: Vec::new(),
        i18n_table: None,
        fast_math: false,
        fp_contract_mode: perry_codegen::FpContractMode::Off,
        app_metadata: AppMetadata::default(),
        namespace_entries: Vec::new(),
        dynamic_import_path_to_prefix: std::collections::HashMap::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
    }
}

fn param(id: u32, name: &str, ty: Type) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
    }
}

fn field(name: &str, ty: Type) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

fn class(id: u32, name: &str, fields: Vec<ClassField>) -> Class {
    Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        fields,
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
    }
}

fn function(
    id: u32,
    name: &str,
    params: Vec<Param>,
    return_type: Type,
    body: Vec<Stmt>,
) -> Function {
    Function {
        id,
        name: name.to_string(),
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

fn module(name: &str, params: Vec<Param>, return_type: Type, body: Vec<Stmt>) -> Module {
    module_with_classes(name, Vec::new(), params, return_type, body)
}

fn module_with_classes(
    name: &str,
    classes: Vec<Class>,
    params: Vec<Param>,
    return_type: Type,
    body: Vec<Stmt>,
) -> Module {
    Module {
        name: name.to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes,
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
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
        }],
        init: Vec::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
    }
}

fn ir_for(module: Module) -> String {
    String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap()
}

fn entry_ir_for(module: Module) -> String {
    let mut opts = empty_opts();
    opts.is_entry_module = true;
    String::from_utf8(compile_module(&module, opts).unwrap()).unwrap()
}

#[test]
fn typed_feedback_trace_dump_runs_before_entry_return() {
    let ir = entry_ir_for(module(
        "typed_feedback_epilogue.ts",
        Vec::new(),
        Type::Void,
        Vec::new(),
    ));

    assert!(ir.contains("declare void @js_typed_feedback_maybe_dump_trace()"));
    let dump_pos = ir
        .rfind("call void @js_typed_feedback_maybe_dump_trace()")
        .expect("entry should call typed-feedback trace dump");
    let ret_pos = ir.rfind("ret i32 0").expect("entry should return i32 0");
    assert!(dump_pos < ret_pos);
}

fn json_stringify_full_expr(value: Expr) -> Expr {
    Expr::JsonStringifyFull(
        Box::new(value),
        Box::new(Expr::Undefined),
        Box::new(Expr::Undefined),
    )
}

#[test]
fn typed_feedback_lowers_length_only_json_stringify_local_to_length_helper() {
    let ir = ir_for(module(
        "typed_feedback_json_stringify_length_only.ts",
        vec![param(1, "value", Type::Any)],
        Type::Number,
        vec![
            Stmt::Let {
                id: 2,
                name: "json".to_string(),
                ty: Type::String,
                mutable: false,
                init: Some(json_stringify_full_expr(Expr::LocalGet(1))),
            },
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(2)),
                property: "length".to_string(),
            })),
        ],
    ));

    assert!(
        ir.contains("call i32 @js_json_stringify_full_length"),
        "{ir}"
    );
    assert!(!ir.contains("call i64 @js_json_stringify_full("), "{ir}");
    assert!(!ir.contains("call double @js_value_length_f64"), "{ir}");
}

#[test]
fn typed_feedback_keeps_json_stringify_local_when_value_escapes() {
    let ir = ir_for(module(
        "typed_feedback_json_stringify_value_escape.ts",
        vec![param(1, "value", Type::Any)],
        Type::String,
        vec![
            Stmt::Let {
                id: 2,
                name: "json".to_string(),
                ty: Type::String,
                mutable: false,
                init: Some(json_stringify_full_expr(Expr::LocalGet(1))),
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));

    assert!(ir.contains("call i64 @js_json_stringify_full("), "{ir}");
    assert!(
        !ir.contains("call i32 @js_json_stringify_full_length"),
        "{ir}"
    );
}

#[test]
fn typed_feedback_folds_constant_modulo_i64_accumulator_loop() {
    let ir = ir_for(module(
        "typed_feedback_modulo_accumulator_closed_form.ts",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "limit".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(10)),
            },
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(5)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Binary {
                            op: BinaryOp::Mod,
                            left: Box::new(Expr::LocalGet(3)),
                            right: Box::new(Expr::Integer(4)),
                        }),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));

    assert!(ir.contains("store i64 18"), "{ir}");
    assert!(!ir.contains("srem"), "{ir}");
    assert!(!ir.contains("for.body"), "{ir}");
    assert!(!ir.contains("asm sideeffect"), "{ir}");
}

#[test]
fn typed_feedback_folds_constant_add_i64_accumulator_loop() {
    let ir = ir_for(module(
        "typed_feedback_constant_accumulator_closed_form.ts",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "limit".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(10)),
            },
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(5)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Integer(2)),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));

    assert!(ir.contains("store i64 25"), "{ir}");
    assert!(!ir.contains("for.body"), "{ir}");
    assert!(!ir.contains("asm sideeffect"), "{ir}");
}

#[test]
fn typed_feedback_keeps_dynamic_bound_constant_add_loop() {
    let ir = ir_for(module(
        "typed_feedback_dynamic_constant_accumulator.ts",
        vec![param(1, "limit", Type::Number)],
        Type::Number,
        vec![
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Integer(1)),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));
    let body_start = ir.find("\nfor.body.").expect("for body block");
    let body_end = ir[body_start..]
        .find("\nfor.update.")
        .map(|offset| body_start + offset)
        .expect("for update block");
    let body_ir = &ir[body_start..body_end];

    assert!(body_ir.contains("fadd double"), "{body_ir}");
    assert!(body_ir.contains("store double"), "{body_ir}");
}

#[test]
fn typed_feedback_keeps_dynamic_modulo_accumulator_loop() {
    let ir = ir_for(module(
        "typed_feedback_dynamic_modulo_accumulator.ts",
        vec![param(1, "limit", Type::Number)],
        Type::Number,
        vec![
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Binary {
                            op: BinaryOp::Mod,
                            left: Box::new(Expr::LocalGet(3)),
                            right: Box::new(Expr::Integer(4)),
                        }),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));

    assert!(ir.contains("for.body"), "{ir}");
    assert!(ir.contains("srem i32"), "{ir}");
}

#[test]
fn typed_feedback_instruments_property_and_method_boundaries() {
    let ir = ir_for(module(
        "typed_feedback_property.ts",
        vec![param(1, "obj", Type::Any)],
        Type::Any,
        vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
                value: Box::new(Expr::Number(1.0)),
            }),
            Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: "run".to_string(),
                }),
                args: vec![Expr::Number(2.0)],
                type_args: Vec::new(),
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
            })),
        ],
    ));

    assert!(ir.contains("@perry_typed_feedback_"));
    assert!(ir.contains("call void @js_typed_feedback_register_site"));
    assert!(ir.contains("object_set_by_name_guard"));
    assert!(ir.contains("object_get_by_name_guard"));
    assert!(ir.contains("method_call_guard"));
    assert!(ir.contains("js_typed_feedback_object_set_field_by_name_fast"));
    assert!(ir.contains("js_object_set_field_by_name"));
    assert!(ir.contains("js_object_get_field_by_name_f64"));
    assert!(ir.contains("call double @js_typed_feedback_native_call_method"));
    assert!(ir.contains("call void @js_typed_feedback_record_guard_pass"));
    assert!(ir.contains("call void @js_typed_feedback_record_guard_fail"));
    assert!(ir.contains("call void @js_typed_feedback_record_fallback_call"));
}

#[test]
fn typed_feedback_guards_direct_class_field_specialization() {
    let point = class(101, "Point", vec![field("x", Type::Number)]);
    let ir = ir_for(module_with_classes(
        "typed_feedback_class_field.ts",
        vec![point],
        vec![param(1, "p", Type::Named("Point".to_string()))],
        Type::Number,
        vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
                value: Box::new(Expr::Number(7.0)),
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
            })),
        ],
    ));

    assert!(ir.contains("class_field_set_guard"));
    assert!(ir.contains("class_field_get_guard"));
    assert!(ir.contains("@perry_typed_shape_raw_f64_mask_"));
    assert!(ir.contains("js_typed_feedback_class_field_set_guard"));
    assert!(ir.contains("js_typed_feedback_class_field_get_guard"));
    assert!(ir.contains("class_field_set.fast"));
    assert!(ir.contains("class_field_set.fallback"));
    assert!(ir.contains("class_field_get.fast"));
    assert!(ir.contains("class_field_get.fallback"));
    assert!(ir.contains("store double"));
    assert!(!ir.contains("call void @js_gc_note_slot_layout"));
    assert!(ir.contains("call void @js_typed_feedback_record_fallback_call"));
    assert!(ir.contains("call void @js_object_set_field_by_name"));
    assert!(ir.contains("call double @js_object_get_field_by_name_f64"));
}

#[test]
fn typed_feedback_guards_direct_class_method_specialization() {
    let mut point = class(103, "Point", vec![field("x", Type::Number)]);
    point.methods.push(Function {
        id: 7,
        name: "inc".to_string(),
        type_params: Vec::new(),
        params: vec![param(2, "n", Type::Number)],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::LocalGet(2)))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let ir = ir_for(module_with_classes(
        "typed_feedback_class_method.ts",
        vec![point],
        vec![param(1, "p", Type::Named("Point".to_string()))],
        Type::Number,
        vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "inc".to_string(),
            }),
            args: vec![Expr::Number(5.0)],
            type_args: Vec::new(),
        }))],
    ));

    assert!(ir.contains("method_direct_call_guard"));
    assert!(ir.contains("js_typed_feedback_method_direct_call_guard"));
    assert!(ir.contains("method_direct.fast"));
    assert!(ir.contains("method_direct.fallback"));
    assert!(ir.contains("call void @js_typed_feedback_record_fallback_call"));
    assert!(ir.contains("call double @js_native_call_method"));
}

#[test]
fn typed_feedback_guards_direct_closure_call_specialization() {
    let closure_ty = Type::Function(FunctionType {
        params: vec![("x".to_string(), Type::Number, false)],
        return_type: Box::new(Type::Number),
        is_async: false,
        is_generator: false,
    });
    let ir = ir_for(module(
        "typed_feedback_closure_call.ts",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 2,
                name: "cb".to_string(),
                ty: closure_ty,
                mutable: false,
                init: Some(Expr::Closure {
                    func_id: 44,
                    params: vec![param(3, "x", Type::Number)],
                    return_type: Type::Number,
                    body: vec![Stmt::Return(Some(Expr::LocalGet(3)))],
                    captures: Vec::new(),
                    mutable_captures: Vec::new(),
                    captures_this: false,
                    enclosing_class: None,
                    is_async: false,
                    is_generator: false,
                    is_strict: false,
                }),
            },
            Stmt::Return(Some(Expr::Call {
                callee: Box::new(Expr::LocalGet(2)),
                args: vec![Expr::Number(9.0)],
                type_args: Vec::new(),
            })),
        ],
    ));

    assert!(ir.contains("closure_direct_call_guard"));
    assert!(ir.contains("js_typed_feedback_closure_direct_call_guard"));
    assert!(ir.contains("closure_direct.fast"));
    assert!(ir.contains("closure_direct.fallback"));
    assert!(ir.contains("call double @perry_closure_"));
    assert!(ir.contains("call double @js_closure_call1"));
    assert!(ir.contains("call void @js_typed_feedback_record_fallback_call"));
}

#[test]
fn typed_feedback_guards_array_index_specialization() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let ir = ir_for(module(
        "typed_feedback_array.ts",
        vec![param(1, "xs", array_ty)],
        Type::Number,
        vec![
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Number(0.0)),
                value: Box::new(Expr::Number(7.0)),
            }),
            Stmt::Return(Some(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Number(0.0)),
            })),
        ],
    ));

    assert!(ir.contains("numeric_array_index_set_guard"));
    assert!(ir.contains("numeric_array_index_get_guard"));
    assert!(ir.contains("js_typed_feedback_numeric_array_index_set_guard"));
    assert!(ir.contains("js_typed_feedback_array_index_set_fallback_boxed"));
    assert!(ir.contains("js_typed_feedback_numeric_array_index_get_guard"));
    assert!(ir.contains("js_typed_feedback_array_index_get_fallback_boxed"));
    assert!(!ir.contains("call i32 @js_array_numeric_set_f64_unboxed"));
    assert!(!ir.contains("call double @js_array_numeric_get_f64_unboxed"));
}

#[test]
fn typed_feedback_preguards_bounded_numeric_array_writes() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let ir = ir_for(module(
        "typed_feedback_range_array_set_preguard.ts",
        vec![param(1, "xs", array_ty.clone())],
        array_ty,
        vec![
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 2,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(1)),
                        property: "length".to_string(),
                    }),
                }),
                update: Some(Expr::Update {
                    id: 2,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::LocalGet(2)),
                    value: Box::new(Expr::LocalGet(2)),
                })],
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    ));

    assert!(ir.contains("range_set_preguard.fast"), "{ir}");
    assert!(ir.contains("idxset.preguarded_numeric_fast"), "{ir}");
    assert!(ir.contains("idxset.preguarded_numeric_fallback"), "{ir}");
    assert!(
        ir.contains("call void @js_typed_feedback_record_array_guard_fast_passes"),
        "{ir}"
    );
    assert_eq!(
        ir.matches("call i32 @js_typed_feedback_numeric_array_index_set_guard")
            .count(),
        1,
        "{ir}"
    );
    assert!(ir.contains("call double @js_typed_feedback_array_index_set_fallback_boxed"));
    assert!(!ir.contains("idxset.bounded_numeric_fast"), "{ir}");
}

#[test]
fn typed_feedback_preguarded_numeric_array_self_add_skips_get_guard() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let ir = ir_for(module(
        "typed_feedback_range_array_numeric_self_add.ts",
        vec![param(1, "xs", array_ty.clone())],
        array_ty,
        vec![
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 2,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(1)),
                        property: "length".to_string(),
                    }),
                }),
                update: Some(Expr::Update {
                    id: 2,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::LocalGet(2)),
                    value: Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(1)),
                            index: Box::new(Expr::LocalGet(2)),
                        }),
                        right: Box::new(Expr::Integer(1)),
                    }),
                })],
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    ));

    assert!(ir.contains("range_set_preguard.fast"), "{ir}");
    assert!(ir.contains("idxset.numeric_f64_self_add.fast"), "{ir}");
    assert!(ir.contains("idxset.numeric_f64_self_add.fallback"), "{ir}");
    assert!(ir.contains("call double @js_typed_feedback_array_index_get_fallback_boxed"));
    assert!(ir.contains("call double @js_dynamic_string_or_number_add"));
    assert_eq!(
        ir.matches("call i32 @js_typed_feedback_numeric_array_index_set_guard")
            .count(),
        1,
        "{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard"),
        "{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard_i32"),
        "{ir}"
    );
}

#[test]
fn typed_feedback_preguards_plain_array_writes_with_length_bound() {
    let array_ty = Type::Array(Box::new(Type::Any));
    let ir = ir_for(module(
        "typed_feedback_plain_array_length_set_preguard.ts",
        vec![
            param(1, "xs", array_ty.clone()),
            param(2, "start", Type::Number),
        ],
        array_ty,
        vec![
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::LocalGet(2)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(1)),
                        property: "length".to_string(),
                    }),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::LocalGet(3)),
                    value: Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(1)),
                            index: Box::new(Expr::LocalGet(3)),
                        }),
                        right: Box::new(Expr::Integer(1)),
                    }),
                })],
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    ));

    assert!(
        ir.contains("call i32 @js_plain_array_inbounds_range_guard"),
        "{ir}"
    );
    assert!(
        ir.contains("call i32 @js_plain_array_inbounds_pointer_free_range_guard"),
        "{ir}"
    );
    assert!(
        ir.contains("call i32 @js_plain_array_f64_number_range_guard"),
        "{ir}"
    );
    assert!(ir.contains("idxset.plain_f64_self_add.fast"), "{ir}");
    assert!(ir.contains("idxset.plain_f64_self_add.fallback"), "{ir}");
    assert!(
        ir.contains("call double @js_typed_feedback_array_index_get_fallback_boxed"),
        "{ir}"
    );
    assert!(
        ir.contains("call double @js_dynamic_string_or_number_add"),
        "{ir}"
    );
    assert!(!ir.contains("idxset.preguarded_plain_layout_note"), "{ir}");
    assert!(!ir.contains("call void @js_gc_note_slot_layout"), "{ir}");
    assert!(!ir.contains("call void @js_write_barrier_slot"), "{ir}");
    assert!(
        !ir.contains("call void @js_array_note_numeric_write"),
        "{ir}"
    );
    assert_eq!(
        ir.matches("call i32 @js_typed_feedback_plain_array_index_set_guard")
            .count(),
        0,
        "{ir}"
    );
}

#[test]
fn typed_feedback_preguards_plain_array_writes_with_local_bound() {
    let array_ty = Type::Array(Box::new(Type::Any));
    let ir = ir_for(module(
        "typed_feedback_plain_array_local_set_preguard.ts",
        vec![
            param(1, "xs", array_ty.clone()),
            param(2, "limit", Type::Number),
        ],
        array_ty,
        vec![
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(2)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::LocalGet(3)),
                    value: Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(1)),
                            index: Box::new(Expr::LocalGet(3)),
                        }),
                        right: Box::new(Expr::Integer(1)),
                    }),
                })],
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    ));

    assert!(
        ir.contains("call i32 @js_plain_array_inbounds_range_guard"),
        "{ir}"
    );
    assert!(
        ir.contains("call i32 @js_plain_array_inbounds_pointer_free_range_guard"),
        "{ir}"
    );
    assert!(
        ir.contains("call i32 @js_plain_array_f64_number_range_guard"),
        "{ir}"
    );
    assert!(ir.contains("idxset.plain_f64_self_add.fast"), "{ir}");
    assert!(ir.contains("idxset.plain_f64_self_add.fallback"), "{ir}");
    assert!(
        ir.contains("call double @js_typed_feedback_array_index_get_fallback_boxed"),
        "{ir}"
    );
    assert!(
        ir.contains("call double @js_dynamic_string_or_number_add"),
        "{ir}"
    );
    assert!(!ir.contains("idxset.preguarded_plain_layout_note"), "{ir}");
    assert!(!ir.contains("call void @js_gc_note_slot_layout"), "{ir}");
    assert!(!ir.contains("call void @js_write_barrier_slot"), "{ir}");
    assert!(
        !ir.contains("call void @js_array_note_numeric_write"),
        "{ir}"
    );
    assert_eq!(
        ir.matches("call i32 @js_typed_feedback_plain_array_index_set_guard")
            .count(),
        0,
        "{ir}"
    );
}

#[test]
fn typed_feedback_guards_numeric_array_push_specialization() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let ir = ir_for(module(
        "typed_feedback_array_push.ts",
        vec![],
        array_ty.clone(),
        vec![
            Stmt::Let {
                id: 1,
                name: "xs".to_string(),
                ty: array_ty,
                mutable: true,
                init: Some(Expr::Array(Vec::new())),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: 1,
                value: Box::new(Expr::Number(7.0)),
            }),
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    ));

    assert!(ir.contains("numeric_array_push_guard"));
    assert!(ir.contains("apush.numeric_inbounds"));
    assert!(!ir.contains("call i32 @js_typed_feedback_numeric_array_push_guard"));
    assert!(!ir.contains("call i64 @js_array_numeric_push_f64_unboxed"));
    assert!(ir.contains("js_typed_feedback_record_fallback_call"));
    assert!(ir.contains("call i64 @js_array_push_f64"));
}

#[test]
fn typed_feedback_marks_numeric_array_literals() {
    let numeric_ir = ir_for(module(
        "typed_feedback_numeric_array_literal.ts",
        Vec::new(),
        Type::Any,
        vec![Stmt::Return(Some(Expr::Array(vec![
            Expr::Number(1.0),
            Expr::Integer(2),
            Expr::Binary {
                op: perry_hir::BinaryOp::Mul,
                left: Box::new(Expr::Number(3.0)),
                right: Box::new(Expr::Number(4.0)),
            },
        ])))],
    ));

    assert!(numeric_ir.contains("call i32 @js_array_mark_numeric_f64_layout"));

    let mixed_ir = ir_for(module(
        "typed_feedback_mixed_array_literal.ts",
        Vec::new(),
        Type::Any,
        vec![Stmt::Return(Some(Expr::Array(vec![
            Expr::Number(1.0),
            Expr::String("x".to_string()),
        ])))],
    ));

    assert!(!mixed_ir.contains("call i32 @js_array_mark_numeric_f64_layout"));
}

#[test]
fn typed_feedback_inline_array_writes_note_numeric_downgrade() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let ir = ir_for(module(
        "typed_feedback_array_numeric_downgrade.ts",
        Vec::new(),
        Type::Any,
        vec![
            Stmt::Let {
                id: 2,
                name: "xs".to_string(),
                ty: array_ty,
                mutable: true,
                init: Some(Expr::Array(vec![Expr::Number(1.0)])),
            },
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(2)),
                index: Box::new(Expr::Number(0.0)),
                value: Box::new(Expr::String("not-number".to_string())),
            }),
            Stmt::Expr(Expr::ArrayPush {
                array_id: 2,
                value: Box::new(Expr::String("still-not-number".to_string())),
            }),
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));

    assert!(ir.contains("call void @js_array_note_numeric_write"));
    assert!(ir.contains("plain_array_index_set_guard"));
    assert!(ir.contains("js_typed_feedback_plain_array_index_set_guard"));
    assert!(!ir.contains("call i32 @js_typed_feedback_numeric_array_index_set_guard"));
}

#[test]
fn typed_feedback_guards_computed_numeric_array_index_hot_path() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let ir = ir_for(module(
        "typed_feedback_computed_array.ts",
        vec![param(1, "xs", array_ty), param(2, "i", Type::Number)],
        Type::Number,
        vec![Stmt::Return(Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Binary {
                op: BinaryOp::Mod,
                left: Box::new(Expr::LocalGet(2)),
                right: Box::new(Expr::Integer(64)),
            }),
        }))],
    ));

    assert!(ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard"));
    assert!(ir.contains("call double @js_typed_feedback_array_index_get_fallback_boxed"));
    assert!(!ir.contains("call double @js_array_numeric_get_f64_unboxed"));
}

#[test]
fn typed_feedback_guards_computed_numeric_array_index_uses_i32_loop_bound() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let ir = ir_for(module(
        "typed_feedback_loop_bound_computed_array.ts",
        vec![param(1, "xs", array_ty), param(2, "size", Type::Number)],
        Type::Number,
        vec![Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: 3,
                name: "i".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            })),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(3)),
                right: Box::new(Expr::LocalGet(2)),
            }),
            update: Some(Expr::Update {
                id: 3,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body: vec![Stmt::Return(Some(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::Binary {
                        op: BinaryOp::Mul,
                        left: Box::new(Expr::LocalGet(3)),
                        right: Box::new(Expr::LocalGet(2)),
                    }),
                    right: Box::new(Expr::Integer(1)),
                }),
            }))],
        }],
    ));

    assert!(ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard_i32"));
    assert!(ir.contains("call double @js_typed_feedback_array_index_get_fallback_boxed"));
    assert!(ir.contains("mul i32"), "{ir}");
    assert!(ir.contains("add i32"), "{ir}");
    assert!(!ir.contains("fmul double"), "{ir}");
    assert!(!ir.contains("call double @js_array_numeric_get_f64_unboxed"));
}

#[test]
fn typed_feedback_guards_computed_numeric_array_write_uses_i32_loop_bound() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let ir = ir_for(module(
        "typed_feedback_loop_bound_computed_array_write.ts",
        vec![
            param(1, "xs", array_ty.clone()),
            param(2, "size", Type::Number),
        ],
        array_ty,
        vec![
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(2)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::Binary {
                            op: BinaryOp::Mul,
                            left: Box::new(Expr::LocalGet(3)),
                            right: Box::new(Expr::LocalGet(2)),
                        }),
                        right: Box::new(Expr::Integer(1)),
                    }),
                    value: Box::new(Expr::LocalGet(3)),
                })],
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    ));

    assert!(ir.contains("call i32 @js_typed_feedback_numeric_array_index_set_guard"));
    assert!(ir.contains("call double @js_typed_feedback_array_index_set_fallback_boxed"));
    assert!(ir.contains("mul i32"), "{ir}");
    assert!(ir.contains("add i32"), "{ir}");
    assert!(!ir.contains("fmul double"), "{ir}");
    assert!(!ir.contains("call i32 @js_array_numeric_set_f64_unboxed"));
}

#[test]
fn numeric_modulo_loop_counter_by_const_uses_i32_srem() {
    let modulo = Expr::Binary {
        op: BinaryOp::Mod,
        left: Box::new(Expr::LocalGet(3)),
        right: Box::new(Expr::Integer(7)),
    };
    let ir = ir_for(module(
        "i32_counter_mod_const.ts",
        vec![param(1, "limit", Type::Number)],
        Type::Number,
        vec![
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(modulo),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));

    assert!(ir.contains("srem i32"), "{ir}");
    assert!(!ir.contains("srem i64"), "{ir}");
}

#[test]
fn numeric_modulo_by_zero_keeps_double_frem_fallback() {
    let modulo = Expr::Binary {
        op: BinaryOp::Mod,
        left: Box::new(Expr::LocalGet(3)),
        right: Box::new(Expr::Integer(0)),
    };
    let ir = ir_for(module(
        "i32_counter_mod_zero.ts",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "limit".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(100)),
            },
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(modulo),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));

    assert!(!ir.contains("srem i32"), "{ir}");
    assert!(!ir.contains("srem i64"), "{ir}");
    assert!(ir.contains("frem double"), "{ir}");
}

#[test]
fn numeric_modulo_by_unsafe_integer_keeps_double_frem_fallback() {
    let modulo = Expr::Binary {
        op: BinaryOp::Mod,
        left: Box::new(Expr::LocalGet(3)),
        right: Box::new(Expr::Number(9_007_199_254_740_992.0)),
    };
    let ir = ir_for(module(
        "i32_counter_mod_unsafe_integer.ts",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "limit".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(100)),
            },
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(modulo),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));

    assert!(!ir.contains("srem i32"), "{ir}");
    assert!(!ir.contains("srem i64"), "{ir}");
    assert!(ir.contains("frem double"), "{ir}");
}

#[test]
fn i32_for_update_skips_per_iteration_double_counter_store() {
    let ir = ir_for(module(
        "i32_for_update_counter.ts",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "limit".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(100)),
            },
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![
                    Stmt::Expr(Expr::LocalSet(
                        2,
                        Box::new(Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(Expr::LocalGet(2)),
                            right: Box::new(Expr::LocalGet(3)),
                        }),
                    )),
                    Stmt::Expr(Expr::LocalGet(2)),
                ],
            },
            Stmt::Return(Some(Expr::LocalGet(3))),
        ],
    ));
    let update_start = ir.find("\nfor.update.").expect("for update block");
    let update_end = ir[update_start..]
        .find("\nfor.exit.")
        .map(|offset| update_start + offset)
        .expect("for exit block");
    let update_ir = &ir[update_start..update_end];
    let exit_ir = &ir[update_end..];

    assert!(update_ir.contains("add i32"), "{update_ir}");
    assert!(!update_ir.contains("fadd double"), "{update_ir}");
    assert!(!update_ir.contains("store double"), "{update_ir}");
    assert!(exit_ir.contains("sitofp i32"), "{exit_ir}");
    assert!(exit_ir.contains("store double"), "{exit_ir}");
}

#[test]
fn i32_for_update_keeps_double_counter_store_for_negative_zero_init() {
    let ir = ir_for(module(
        "i32_for_update_negative_zero_counter.ts",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "limit".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 2,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Number(-0.0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 2,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: Vec::new(),
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));
    let update_start = ir.find("\nfor.update.").expect("for update block");
    let update_end = ir[update_start..]
        .find("\nfor.exit.")
        .map(|offset| update_start + offset)
        .expect("for exit block");
    let update_ir = &ir[update_start..update_end];
    let exit_ir = &ir[update_end..];

    assert!(update_ir.contains("fadd double"), "{update_ir}");
    assert!(update_ir.contains("store double"), "{update_ir}");
    assert!(!exit_ir.contains("sitofp i32"), "{exit_ir}");
}

#[test]
fn i64_loop_accumulator_syncs_once_for_self_add_body() {
    let modulo = Expr::Binary {
        op: BinaryOp::Mod,
        left: Box::new(Expr::LocalGet(3)),
        right: Box::new(Expr::Integer(10)),
    };
    let ir = ir_for(module(
        "i64_loop_accumulator.ts",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "limit".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(100)),
            },
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(modulo),
                            right: Box::new(Expr::Integer(0)),
                        }),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));
    let body_start = ir.find("\nfor.body.").expect("for body block");
    let body_end = ir[body_start..]
        .find("\nfor.update.")
        .map(|offset| body_start + offset)
        .expect("for update block");
    let exit_start = ir.find("\nfor.exit.").expect("for exit block");
    let body_ir = &ir[body_start..body_end];
    let exit_ir = &ir[exit_start..];

    assert!(body_ir.contains("srem i32"), "{body_ir}");
    assert!(body_ir.contains("add i64"), "{body_ir}");
    assert!(body_ir.contains("store i64"), "{body_ir}");
    assert!(!body_ir.contains("fadd double"), "{body_ir}");
    assert!(!body_ir.contains("store double"), "{body_ir}");
    assert!(exit_ir.contains("sitofp i64"), "{exit_ir}");
    assert!(exit_ir.contains("store double"), "{exit_ir}");
}

#[test]
fn i64_loop_accumulator_uses_uint8array_byte_addend() {
    let byte_get = Expr::Uint8ArrayGet {
        array: Box::new(Expr::LocalGet(2)),
        index: Box::new(Expr::LocalGet(4)),
    };
    let ir = ir_for(module(
        "i64_loop_accumulator_uint8array_get.ts",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "size".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(16)),
            },
            Stmt::Let {
                id: 2,
                name: "bytes".to_string(),
                ty: Type::Named("Uint8Array".to_string()),
                mutable: false,
                init: Some(Expr::Uint8ArrayNew(Some(Box::new(Expr::LocalGet(1))))),
            },
            Stmt::Let {
                id: 3,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 4,
                    name: "i".to_string(),
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
                body: vec![Stmt::Expr(Expr::LocalSet(
                    3,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(3)),
                        right: Box::new(byte_get),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(3))),
        ],
    ));
    let body_start = ir.find("\nfor.body.").expect("for body block");
    let body_end = ir[body_start..]
        .find("\nfor.update.")
        .map(|offset| body_start + offset)
        .expect("for update block");
    let exit_start = ir.find("\nfor.exit.").expect("for exit block");
    let body_ir = &ir[body_start..body_end];
    let exit_ir = &ir[exit_start..];

    assert!(body_ir.contains("load i8"), "{body_ir}");
    assert!(body_ir.contains("zext i8"), "{body_ir}");
    assert!(body_ir.contains("zext i32"), "{body_ir}");
    assert!(body_ir.contains("add i64"), "{body_ir}");
    assert!(body_ir.contains("store i64"), "{body_ir}");
    assert!(!body_ir.contains("fadd double"), "{body_ir}");
    assert!(!body_ir.contains("store double"), "{body_ir}");
    assert!(
        !body_ir.contains("call i32 @js_uint8array_get"),
        "{body_ir}"
    );
    assert!(exit_ir.contains("sitofp i64"), "{exit_ir}");
    assert!(exit_ir.contains("store double"), "{exit_ir}");
}

#[test]
fn typed_feedback_folds_affine_i64_accumulator_loop() {
    let affine = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::Binary {
            op: BinaryOp::Mul,
            left: Box::new(Expr::LocalGet(3)),
            right: Box::new(Expr::Integer(2)),
        }),
        right: Box::new(Expr::Integer(1)),
    };
    let ir = ir_for(module(
        "i64_loop_accumulator_affine_addend.ts",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "limit".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(10)),
            },
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(5)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(2)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(affine),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));

    assert!(ir.contains("store i64 101"), "{ir}");
    assert!(ir.contains("store i32 10"), "{ir}");
    assert!(!ir.contains("mul i64"), "{ir}");
    assert!(!ir.contains("for.body"), "{ir}");
    assert!(!ir.contains("asm sideeffect"), "{ir}");
}

#[test]
fn typed_feedback_keeps_dynamic_bound_affine_accumulator_loop() {
    let affine = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::Binary {
            op: BinaryOp::Mul,
            left: Box::new(Expr::LocalGet(3)),
            right: Box::new(Expr::Integer(2)),
        }),
        right: Box::new(Expr::Integer(1)),
    };
    let ir = ir_for(module(
        "typed_feedback_dynamic_affine_accumulator.ts",
        vec![param(1, "limit", Type::Number)],
        Type::Number,
        vec![
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(affine),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));
    let body_start = ir.find("\nfor.body.").expect("for body block");
    let body_end = ir[body_start..]
        .find("\nfor.update.")
        .map(|offset| body_start + offset)
        .expect("for update block");
    let body_ir = &ir[body_start..body_end];

    assert!(body_ir.contains("fadd double"), "{body_ir}");
    assert!(body_ir.contains("store double"), "{body_ir}");
}

#[test]
fn typed_feedback_folds_direct_class_field_accumulator_loop() {
    let mut counter = class(120, "Counter", vec![field("value", Type::Number)]);
    counter.constructor = Some(function(
        121,
        "constructor",
        Vec::new(),
        Type::Void,
        vec![Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: "value".to_string(),
            value: Box::new(Expr::Integer(5)),
        })],
    ));
    counter.methods.push(function(
        124,
        "increment",
        Vec::new(),
        Type::Void,
        vec![Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: "value".to_string(),
            value: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::This),
                    property: "value".to_string(),
                }),
                right: Box::new(Expr::Integer(1)),
            }),
        })],
    ));
    counter.methods.push(function(
        125,
        "get",
        Vec::new(),
        Type::Number,
        vec![Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::This),
            property: "value".to_string(),
        }))],
    ));
    let ir = ir_for(module_with_classes(
        "direct_class_field_accumulator_closed_form.ts",
        vec![counter],
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "limit".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(10)),
            },
            Stmt::Let {
                id: 2,
                name: "counter".to_string(),
                ty: Type::Named("Counter".to_string()),
                mutable: false,
                init: Some(Expr::New {
                    class_name: "Counter".to_string(),
                    args: Vec::new(),
                    type_args: Vec::new(),
                }),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(2)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::LocalGet(2)),
                    property: "value".to_string(),
                    value: Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(2)),
                            property: "value".to_string(),
                        }),
                        right: Box::new(Expr::Integer(1)),
                    }),
                })],
            },
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(2)),
                property: "value".to_string(),
            })),
        ],
    ));

    assert!(ir.contains("store double 13.0"), "{ir}");
    assert!(ir.contains("store i32 10"), "{ir}");
    assert!(!ir.contains("for.body"), "{ir}");
    assert!(!ir.contains("asm sideeffect"), "{ir}");
}

#[test]
fn typed_feedback_keeps_dynamic_bound_direct_class_field_accumulator_loop() {
    let mut counter = class(122, "Counter", vec![field("value", Type::Number)]);
    counter.constructor = Some(function(
        123,
        "constructor",
        Vec::new(),
        Type::Void,
        vec![Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: "value".to_string(),
            value: Box::new(Expr::Integer(0)),
        })],
    ));
    counter.methods.push(function(
        126,
        "increment",
        Vec::new(),
        Type::Void,
        vec![Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: "value".to_string(),
            value: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::This),
                    property: "value".to_string(),
                }),
                right: Box::new(Expr::Integer(1)),
            }),
        })],
    ));
    counter.methods.push(function(
        127,
        "get",
        Vec::new(),
        Type::Number,
        vec![Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::This),
            property: "value".to_string(),
        }))],
    ));
    let ir = ir_for(module_with_classes(
        "direct_class_field_accumulator_dynamic_bound.ts",
        vec![counter],
        vec![param(1, "limit", Type::Number)],
        Type::Number,
        vec![
            Stmt::Let {
                id: 2,
                name: "counter".to_string(),
                ty: Type::Named("Counter".to_string()),
                mutable: false,
                init: Some(Expr::New {
                    class_name: "Counter".to_string(),
                    args: Vec::new(),
                    type_args: Vec::new(),
                }),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::LocalGet(2)),
                    property: "value".to_string(),
                    value: Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(2)),
                            property: "value".to_string(),
                        }),
                        right: Box::new(Expr::Integer(1)),
                    }),
                })],
            },
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(2)),
                property: "value".to_string(),
            })),
        ],
    ));
    let body_start = ir.find("\nfor.body.").expect("for body block");
    let body_end = ir[body_start..]
        .find("\nfor.update.")
        .map(|offset| body_start + offset)
        .expect("for update block");
    let body_ir = &ir[body_start..body_end];

    assert!(body_ir.contains("fadd double"), "{body_ir}");
    assert!(body_ir.contains("store double"), "{body_ir}");
}

#[test]
fn integer_specialized_fibonacci_recurrence_uses_i64_loop() {
    let ir = ir_for(module(
        "integer_fibonacci_loop.ts",
        vec![param(2, "n", Type::Number)],
        Type::Number,
        vec![
            Stmt::If {
                condition: Expr::Compare {
                    op: CompareOp::Le,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::Integer(1)),
                },
                then_branch: vec![Stmt::Return(Some(Expr::LocalGet(2)))],
                else_branch: None,
            },
            Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::Call {
                    callee: Box::new(Expr::FuncRef(1)),
                    args: vec![Expr::Binary {
                        op: BinaryOp::Sub,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Integer(1)),
                    }],
                    type_args: Vec::new(),
                }),
                right: Box::new(Expr::Call {
                    callee: Box::new(Expr::FuncRef(1)),
                    args: vec![Expr::Binary {
                        op: BinaryOp::Sub,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Integer(2)),
                    }],
                    type_args: Vec::new(),
                }),
            })),
        ],
    ));
    let i64_name = "perry_fn_integer_fibonacci_loop_ts__probe_i64";
    let wrapper_name = "perry_fn_integer_fibonacci_loop_ts__probe";

    assert!(
        ir.contains(&format!("define i64 @{i64_name}(i64 %arg2) alwaysinline")),
        "{ir}"
    );
    assert!(
        ir.contains(&format!(
            "define double @{wrapper_name}(double %arg2) alwaysinline"
        )),
        "{ir}"
    );
    assert!(ir.contains("i64.fib.loop"), "{ir}");
    assert!(ir.contains("i64.fib.done"), "{ir}");
    assert!(ir.contains("icmp sle i64 %arg2, 1"), "{ir}");
    assert_eq!(ir.matches(&format!("call i64 @{i64_name}")).count(), 1);
    assert!(
        !ir.contains(&format!("call fastcc i64 @{i64_name}")),
        "{ir}"
    );
}

#[test]
fn i64_loop_accumulator_rejects_negative_zero_initial_value() {
    let ir = ir_for(module(
        "i64_loop_accumulator_negative_zero.ts",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "limit".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(2)),
            },
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(-0.0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Integer(1)),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));
    let body_start = ir.find("\nfor.body.").expect("for body block");
    let body_end = ir[body_start..]
        .find("\nfor.update.")
        .map(|offset| body_start + offset)
        .expect("for update block");
    let body_ir = &ir[body_start..body_end];

    assert!(!body_ir.contains("add i64"), "{body_ir}");
    assert!(!body_ir.contains("store i64"), "{body_ir}");
    assert!(body_ir.contains("fadd double"), "{body_ir}");
    assert!(body_ir.contains("store double"), "{body_ir}");
}

#[test]
fn i64_loop_accumulator_rejects_growth_past_safe_integer() {
    let ir = ir_for(module(
        "i64_loop_accumulator_overflow.ts",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "limit".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(2)),
            },
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(9_007_199_254_740_991)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::LocalGet(1)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Integer(1)),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));
    let body_start = ir.find("\nfor.body.").expect("for body block");
    let body_end = ir[body_start..]
        .find("\nfor.update.")
        .map(|offset| body_start + offset)
        .expect("for update block");
    let body_ir = &ir[body_start..body_end];

    assert!(!body_ir.contains("add i64"), "{body_ir}");
    assert!(!body_ir.contains("store i64"), "{body_ir}");
    assert!(body_ir.contains("fadd double"), "{body_ir}");
    assert!(body_ir.contains("store double"), "{body_ir}");
}

#[test]
fn typed_feedback_preguards_modulo_numeric_array_reads_in_local_bound_loop() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let first_read = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::Binary {
            op: BinaryOp::Mod,
            left: Box::new(Expr::LocalGet(5)),
            right: Box::new(Expr::LocalGet(2)),
        }),
    };
    let second_read = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::Binary {
            op: BinaryOp::Mod,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(Expr::LocalGet(5)),
                right: Box::new(Expr::Integer(7)),
            }),
            right: Box::new(Expr::LocalGet(2)),
        }),
    };
    let ir = ir_for(module(
        "typed_feedback_modulo_array_preguard.ts",
        vec![param(1, "xs", array_ty)],
        Type::Number,
        vec![
            Stmt::Let {
                id: 2,
                name: "n".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(64)),
            },
            Stmt::Let {
                id: 3,
                name: "limit".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(1000)),
            },
            Stmt::Let {
                id: 4,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(1)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 5,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(5)),
                    right: Box::new(Expr::LocalGet(3)),
                }),
                update: Some(Expr::Update {
                    id: 5,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    4,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::Binary {
                            op: BinaryOp::Mul,
                            left: Box::new(Expr::LocalGet(4)),
                            right: Box::new(first_read),
                        }),
                        right: Box::new(second_read),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(4))),
        ],
    ));

    assert!(ir.contains("modulo_preguard.fast"), "{ir}");
    assert!(ir.contains("bidx.preguarded.fast"), "{ir}");
    assert!(ir.contains("bidx.preguarded.fallback"), "{ir}");
    assert!(
        ir.contains("call void @js_typed_feedback_record_array_guard_fast_passes"),
        "{ir}"
    );
    assert_eq!(
        ir.matches("call i32 @js_typed_feedback_numeric_array_index_get_guard_i32")
            .count(),
        2,
        "{ir}"
    );
    assert!(!ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard("));
    assert!(ir.contains("srem i32"), "{ir}");
    assert!(!ir.contains("srem i64"), "{ir}");
    assert!(!ir.contains("arr.fast"), "{ir}");
}

#[test]
fn typed_feedback_preguards_affine_numeric_array_reads_in_local_bound_loop() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let a_ik = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(Expr::LocalGet(5)),
                right: Box::new(Expr::LocalGet(3)),
            }),
            right: Box::new(Expr::LocalGet(7)),
        }),
    };
    let b_kj = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(2)),
        index: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(Expr::LocalGet(7)),
                right: Box::new(Expr::LocalGet(3)),
            }),
            right: Box::new(Expr::LocalGet(6)),
        }),
    };
    let ir = ir_for(module(
        "typed_feedback_affine_array_preguard.ts",
        vec![
            param(1, "a", array_ty.clone()),
            param(2, "b", array_ty),
            param(3, "size", Type::Number),
        ],
        Type::Number,
        vec![
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 5,
                    name: "i".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(5)),
                    right: Box::new(Expr::LocalGet(3)),
                }),
                update: Some(Expr::Update {
                    id: 5,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::For {
                    init: Some(Box::new(Stmt::Let {
                        id: 6,
                        name: "j".to_string(),
                        ty: Type::Any,
                        mutable: true,
                        init: Some(Expr::Integer(0)),
                    })),
                    condition: Some(Expr::Compare {
                        op: CompareOp::Lt,
                        left: Box::new(Expr::LocalGet(6)),
                        right: Box::new(Expr::LocalGet(3)),
                    }),
                    update: Some(Expr::Update {
                        id: 6,
                        op: UpdateOp::Increment,
                        prefix: false,
                    }),
                    body: vec![Stmt::For {
                        init: Some(Box::new(Stmt::Let {
                            id: 7,
                            name: "k".to_string(),
                            ty: Type::Number,
                            mutable: true,
                            init: Some(Expr::Integer(0)),
                        })),
                        condition: Some(Expr::Compare {
                            op: CompareOp::Lt,
                            left: Box::new(Expr::LocalGet(7)),
                            right: Box::new(Expr::LocalGet(3)),
                        }),
                        update: Some(Expr::Update {
                            id: 7,
                            op: UpdateOp::Increment,
                            prefix: false,
                        }),
                        body: vec![Stmt::Expr(Expr::LocalSet(
                            4,
                            Box::new(Expr::Binary {
                                op: BinaryOp::Add,
                                left: Box::new(Expr::LocalGet(4)),
                                right: Box::new(Expr::Binary {
                                    op: BinaryOp::Mul,
                                    left: Box::new(a_ik),
                                    right: Box::new(b_kj),
                                }),
                            }),
                        ))],
                    }],
                }],
            },
            Stmt::Return(Some(Expr::LocalGet(4))),
        ],
    ));

    assert!(ir.contains("affine_preguard.fast"), "{ir}");
    assert!(ir.contains("bidx.preguarded.fast"), "{ir}");
    assert!(ir.contains("bidx.preguarded.fallback"), "{ir}");
    assert!(
        ir.contains("call void @js_typed_feedback_record_array_guard_fast_passes"),
        "{ir}"
    );
    assert!(ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard_i32"));
    assert!(!ir.contains("arr.fast"), "{ir}");
}

#[test]
fn typed_feedback_preguards_affine_numeric_array_writes_in_local_bound_loop() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let a_ik = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(Expr::LocalGet(5)),
                right: Box::new(Expr::LocalGet(3)),
            }),
            right: Box::new(Expr::LocalGet(7)),
        }),
    };
    let b_kj = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(2)),
        index: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(Expr::LocalGet(7)),
                right: Box::new(Expr::LocalGet(3)),
            }),
            right: Box::new(Expr::LocalGet(6)),
        }),
    };
    let c_ij = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::Binary {
            op: BinaryOp::Mul,
            left: Box::new(Expr::LocalGet(5)),
            right: Box::new(Expr::LocalGet(3)),
        }),
        right: Box::new(Expr::LocalGet(6)),
    };
    let ir = ir_for(module(
        "typed_feedback_affine_array_set_preguard.ts",
        vec![
            param(1, "a", array_ty.clone()),
            param(2, "b", array_ty.clone()),
            param(3, "size", Type::Number),
            param(8, "c", array_ty.clone()),
        ],
        array_ty,
        vec![
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 5,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(5)),
                    right: Box::new(Expr::LocalGet(3)),
                }),
                update: Some(Expr::Update {
                    id: 5,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::For {
                    init: Some(Box::new(Stmt::Let {
                        id: 6,
                        name: "j".to_string(),
                        ty: Type::Number,
                        mutable: true,
                        init: Some(Expr::Integer(0)),
                    })),
                    condition: Some(Expr::Compare {
                        op: CompareOp::Lt,
                        left: Box::new(Expr::LocalGet(6)),
                        right: Box::new(Expr::LocalGet(3)),
                    }),
                    update: Some(Expr::Update {
                        id: 6,
                        op: UpdateOp::Increment,
                        prefix: false,
                    }),
                    body: vec![
                        Stmt::Let {
                            id: 4,
                            name: "sum".to_string(),
                            ty: Type::Number,
                            mutable: true,
                            init: Some(Expr::Integer(0)),
                        },
                        Stmt::For {
                            init: Some(Box::new(Stmt::Let {
                                id: 7,
                                name: "k".to_string(),
                                ty: Type::Any,
                                mutable: true,
                                init: Some(Expr::Integer(0)),
                            })),
                            condition: Some(Expr::Compare {
                                op: CompareOp::Lt,
                                left: Box::new(Expr::LocalGet(7)),
                                right: Box::new(Expr::LocalGet(3)),
                            }),
                            update: Some(Expr::Update {
                                id: 7,
                                op: UpdateOp::Increment,
                                prefix: false,
                            }),
                            body: vec![Stmt::Expr(Expr::LocalSet(
                                4,
                                Box::new(Expr::Binary {
                                    op: BinaryOp::Add,
                                    left: Box::new(Expr::LocalGet(4)),
                                    right: Box::new(Expr::Binary {
                                        op: BinaryOp::Mul,
                                        left: Box::new(a_ik),
                                        right: Box::new(b_kj),
                                    }),
                                }),
                            ))],
                        },
                        Stmt::Expr(Expr::IndexSet {
                            object: Box::new(Expr::LocalGet(8)),
                            index: Box::new(c_ij),
                            value: Box::new(Expr::LocalGet(4)),
                        }),
                    ],
                }],
            },
            Stmt::Return(Some(Expr::LocalGet(8))),
        ],
    ));

    assert!(ir.contains("affine_set_preguard.fast"), "{ir}");
    assert!(ir.contains("idxset.preguarded_numeric_fast"), "{ir}");
    assert!(ir.contains("idxset.preguarded_numeric_fallback"), "{ir}");
    assert!(ir.contains("call i32 @js_typed_feedback_numeric_array_index_set_guard"));
    assert!(!ir.contains("idxset.guarded"), "{ir}");
}

#[test]
fn typed_feedback_hoists_invariant_numeric_array_get_out_of_inner_loop() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let arr_i = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::LocalGet(3)),
    };
    let arr_j = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::LocalGet(4)),
    };
    let ir = ir_for(module(
        "typed_feedback_nested_array_hoist.ts",
        vec![param(1, "xs", array_ty)],
        Type::Number,
        vec![
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(1)),
                        property: "length".to_string(),
                    }),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::For {
                    init: Some(Box::new(Stmt::Let {
                        id: 4,
                        name: "j".to_string(),
                        ty: Type::Number,
                        mutable: true,
                        init: Some(Expr::Integer(0)),
                    })),
                    condition: Some(Expr::Compare {
                        op: CompareOp::Lt,
                        left: Box::new(Expr::LocalGet(4)),
                        right: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(1)),
                            property: "length".to_string(),
                        }),
                    }),
                    update: Some(Expr::Update {
                        id: 4,
                        op: UpdateOp::Increment,
                        prefix: false,
                    }),
                    body: vec![Stmt::Expr(Expr::LocalSet(
                        2,
                        Box::new(Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(Expr::Binary {
                                op: BinaryOp::Add,
                                left: Box::new(Expr::LocalGet(2)),
                                right: Box::new(arr_i),
                            }),
                            right: Box::new(arr_j),
                        }),
                    ))],
                }],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));

    assert!(ir.contains("for.prebody"), "{ir}");
    assert!(ir.contains("hoist.num.fast"), "{ir}");
    assert!(ir.contains("range_preguard.fast"), "{ir}");
    assert!(ir.contains("bidx.preguarded.fast"), "{ir}");
    assert!(ir.contains("bidx.preguarded.fallback"), "{ir}");
    assert!(
        ir.contains("call void @js_typed_feedback_record_array_guard_fast_passes"),
        "{ir}"
    );
    assert!(ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard_i32"));
    assert!(!ir.contains("bidx.num.fast"), "{ir}");
}

#[test]
fn typed_feedback_does_not_hoist_branch_only_invariant_numeric_array_get() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let arr_i = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::LocalGet(3)),
    };
    let arr_j = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::LocalGet(4)),
    };
    let ir = ir_for(module(
        "typed_feedback_nested_array_no_conditional_hoist.ts",
        vec![param(1, "xs", array_ty)],
        Type::Number,
        vec![
            Stmt::Let {
                id: 2,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 3,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(1)),
                        property: "length".to_string(),
                    }),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::For {
                    init: Some(Box::new(Stmt::Let {
                        id: 4,
                        name: "j".to_string(),
                        ty: Type::Number,
                        mutable: true,
                        init: Some(Expr::Integer(0)),
                    })),
                    condition: Some(Expr::Compare {
                        op: CompareOp::Lt,
                        left: Box::new(Expr::LocalGet(4)),
                        right: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(1)),
                            property: "length".to_string(),
                        }),
                    }),
                    update: Some(Expr::Update {
                        id: 4,
                        op: UpdateOp::Increment,
                        prefix: false,
                    }),
                    body: vec![Stmt::Expr(Expr::LocalSet(
                        2,
                        Box::new(Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(Expr::LocalGet(2)),
                            right: Box::new(Expr::Conditional {
                                condition: Box::new(Expr::Bool(false)),
                                then_expr: Box::new(arr_i),
                                else_expr: Box::new(arr_j),
                            }),
                        }),
                    ))],
                }],
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));

    assert!(!ir.contains("for.prebody"), "{ir}");
    assert!(!ir.contains("hoist.num.fast"), "{ir}");
    assert!(
        !ir.contains("call void @js_typed_feedback_record_array_guard_fast_passes"),
        "{ir}"
    );
}
