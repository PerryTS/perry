use crate::{compile_module, CompileOptions, ImportedClass};
use perry_hir::types::Type;
use perry_hir::{
    Class, ClassField, Expr, Function, Interface, InterfaceProperty, Module, Param, Stmt,
};

fn number_param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Number,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn has_method() -> Function {
    Function {
        id: 2,
        name: "hasComponent".to_string(),
        type_params: Vec::new(),
        params: vec![number_param(1, "componentType")],
        return_type: Type::Boolean,
        body: vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::This),
                    property: "componentTypeSet".to_string(),
                    byte_offset: 0,
                }),
                property: "has".to_string(),
                byte_offset: 0,
            }),
            args: vec![Expr::LocalGet(1)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn archetype_class() -> Class {
    Class {
        id: 1,
        name: "Archetype".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            name: "componentTypeSet".to_string(),
            key_expr: None,
            ty: Type::Generic {
                base: "ReadonlySet".to_string(),
                type_args: vec![Type::Number],
            },
            init: None,
            is_private: false,
            is_readonly: true,
            decorators: Vec::new(),
        }],
        constructor: None,
        methods: vec![has_method()],
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

fn executor_class() -> Class {
    Class {
        id: 3,
        name: "CommandExecutor".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: vec![Function {
            id: 4,
            name: "contains".to_string(),
            type_params: Vec::new(),
            params: vec![
                Param {
                    id: 1,
                    name: "archetype".to_string(),
                    ty: Type::Union(vec![Type::Named("Archetype".to_string()), Type::Void]),
                    default: None,
                    decorators: Vec::new(),
                    is_rest: false,
                    arguments_object: None,
                },
                number_param(2, "componentType"),
            ],
            return_type: Type::Boolean,
            body: vec![Stmt::Return(Some(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(1)),
                        property: "componentTypeSet".to_string(),
                        byte_offset: 0,
                    }),
                    property: "has".to_string(),
                    byte_offset: 0,
                }),
                args: vec![Expr::LocalGet(2)],
                type_args: Vec::new(),
                byte_offset: 0,
            }))],
            is_async: false,
            is_generator: false,
            is_strict: true,
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
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

fn compile_has_ir() -> String {
    let mut module = Module::new("readonly_set_field.ts");
    module.classes.push(archetype_class());
    module.classes.push(executor_class());
    String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            },
        )
        .expect("ReadonlySet field call compiles"),
    )
    .expect("LLVM IR is UTF-8")
}

fn imported_archetype() -> ImportedClass {
    ImportedClass {
        name: "Archetype".to_string(),
        local_alias: None,
        namespace: None,
        source_prefix: "archetype_ts".to_string(),
        constructor_param_count: 0,
        has_own_constructor: true,
        constructor_has_rest: false,
        has_instance_fields: true,
        method_names: Vec::new(),
        proven_this_method_names: Vec::new(),
        proven_this_tower_method_names: Vec::new(),
        method_return_types: Vec::new(),
        method_param_counts: Vec::new(),
        method_has_rest: Vec::new(),
        method_has_synthetic_arguments: Vec::new(),
        method_arguments_length_only: Vec::new(),
        static_field_names: Vec::new(),
        static_method_names: Vec::new(),
        static_method_return_types: Vec::new(),
        static_method_param_counts: Vec::new(),
        static_method_has_rest: Vec::new(),
        static_method_has_user_rest: Vec::new(),
        static_method_has_synthetic_arguments: Vec::new(),
        getter_names: Vec::new(),
        getter_return_types: Vec::new(),
        setter_names: Vec::new(),
        parent_name: None,
        field_names: vec!["componentTypeSet".to_string()],
        field_types: vec![Type::Generic {
            base: "ReadonlySet".to_string(),
            type_args: vec![Type::Number],
        }],
        source_class_id: Some(1),
        return_shape_imports: Vec::new(),
        object_literal: None,
    }
}

fn compile_imported_has_ir() -> String {
    let mut module = Module::new("command_executor.ts");
    module.classes.push(executor_class());
    let mut options = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    options.imported_classes.push(imported_archetype());
    String::from_utf8(
        compile_module(&module, options).expect("imported ReadonlySet field compiles"),
    )
    .expect("LLVM IR is UTF-8")
}

fn compile_nested_map_get_ir() -> String {
    let mut module = Module::new("command_executor.ts");
    module.interfaces.push(Interface {
        id: 1,
        name: "CommandExecutorContext".to_string(),
        type_params: Vec::new(),
        extends: Vec::new(),
        properties: vec![InterfaceProperty {
            name: "entityToArchetype".to_string(),
            ty: Type::Generic {
                base: "Map".to_string(),
                type_args: vec![Type::Number, Type::Number],
            },
            optional: false,
            readonly: false,
        }],
        methods: Vec::new(),
        is_exported: false,
    });
    module.classes.push(Class {
        id: 2,
        name: "CommandExecutor".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            name: "ctx".to_string(),
            key_expr: None,
            ty: Type::Named("CommandExecutorContext".to_string()),
            init: None,
            is_private: false,
            is_readonly: true,
            decorators: Vec::new(),
        }],
        constructor: None,
        methods: vec![Function {
            id: 3,
            name: "lookup".to_string(),
            type_params: Vec::new(),
            params: vec![number_param(1, "entityId")],
            return_type: Type::Number,
            body: vec![Stmt::Return(Some(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::This),
                            property: "ctx".to_string(),
                            byte_offset: 0,
                        }),
                        property: "entityToArchetype".to_string(),
                        byte_offset: 0,
                    }),
                    property: "get".to_string(),
                    byte_offset: 0,
                }),
                args: vec![Expr::LocalGet(1)],
                type_args: Vec::new(),
                byte_offset: 0,
            }))],
            is_async: false,
            is_generator: false,
            is_strict: true,
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
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    });

    String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            },
        )
        .expect("nested declared Map.get compiles"),
    )
    .expect("LLVM IR is UTF-8")
}

fn method_ir<'a>(ir: &'a str, owner: &str, method: &str) -> &'a str {
    let suffix = format!("__{owner}__{method}(");
    let suffix_start = ir.find(&suffix).expect("requested method is present");
    let start = ir[..suffix_start]
        .rfind("define double @perry_method_")
        .expect("requested method has a definition");
    let method_and_rest = &ir[start..];
    let end = method_and_rest
        .find("\n}\n")
        .expect("requested method has a closing brace");
    &method_and_rest[..end + 3]
}

#[test]
fn readonly_set_field_has_uses_branded_collection_fast_path() {
    let ir = compile_has_ir();
    let method_ir = method_ir(&ir, "Archetype", "hasComponent");

    assert!(
        method_ir.contains("call double @js_readonly_set_has("),
        "ReadonlySet.has must use the branded native-Set fast path with a structural-object fallback:\n{method_ir}"
    );
    assert!(
        !method_ir.contains("call double @js_typed_feedback_native_call_method_by_id("),
        "the common native-Set case must not enter the full generic dispatch tower:\n{method_ir}"
    );
}

#[test]
fn nullable_class_receiver_readonly_set_field_uses_branded_fast_path() {
    let ir = compile_has_ir();
    let method_ir = method_ir(&ir, "CommandExecutor", "contains");

    assert!(
        method_ir.contains("call double @js_readonly_set_has("),
        "a ReadonlySet field reached through `Archetype | undefined` must retain the branded candidate:\n{method_ir}"
    );
    assert!(
        !method_ir.contains("call double @js_typed_feedback_native_call_method_by_id("),
        "the nullable owner type must not force every native Set through generic method dispatch:\n{method_ir}"
    );
}

#[test]
fn imported_class_readonly_set_field_uses_branded_fast_path() {
    let ir = compile_imported_has_ir();
    let method_ir = method_ir(&ir, "CommandExecutor", "contains");

    assert!(
        method_ir.contains("call double @js_readonly_set_has("),
        "an imported class's published ReadonlySet field type must remain a branded candidate:\n{method_ir}"
    );
    assert!(
        !method_ir.contains("call double @js_typed_feedback_native_call_method_by_id("),
        "cross-module field metadata must not force native Sets through generic dispatch:\n{method_ir}"
    );
}

#[test]
fn nested_interface_map_field_get_uses_branded_dispatch() {
    let ir = compile_nested_map_get_ir();
    let method_ir = method_ir(&ir, "CommandExecutor", "lookup");

    assert!(
        method_ir.contains("call double @js_declared_map_get("),
        "a Map reached through a nested interface field must retain a branded dispatch candidate:\n{method_ir}"
    );
    assert!(
        !method_ir.contains("call double @js_native_call_method_by_id("),
        "a genuine native Map must not enter generic method dispatch at this site:\n{method_ir}"
    );
}
