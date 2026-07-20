use super::*;
use std::collections::HashMap;

fn empty_env() -> HirTypeEnv {
    HirTypeEnv::new()
}

fn function_type(return_type: Type) -> Type {
    Type::Function(perry_types::FunctionType {
        params: Vec::new(),
        return_type: Box::new(return_type),
        is_async: false,
        is_generator: false,
    })
}

fn function_decl(id: FuncId, name: &str, return_type: Type) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type,
        body: Vec::new(),
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

fn class_field(name: &str, ty: Type) -> ClassField {
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

fn closure_returning(return_type: Type) -> Expr {
    Expr::Closure {
        func_id: 99,
        params: Vec::new(),
        return_type,
        body: Vec::new(),
        captures: Vec::new(),
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    }
}

#[test]
fn infers_basic_hir_expression_types() {
    assert_eq!(
        infer_expr_type(&Expr::Number(1.0), &empty_env()),
        Type::Number
    );
    assert_eq!(
        infer_expr_type(&Expr::Bool(true), &empty_env()),
        Type::Boolean
    );
    assert_eq!(
        infer_expr_type(&Expr::String("x".to_string()), &empty_env()),
        Type::String
    );
    assert_eq!(
        infer_expr_type(&Expr::SymbolNew(None), &empty_env()),
        Type::Symbol
    );
    assert_eq!(
        infer_expr_type(
            &Expr::SymbolFor(Box::new(Expr::String("key".to_string()))),
            &empty_env(),
        ),
        Type::Symbol
    );
}

#[test]
fn uses_environment_for_local_and_function_return_types() {
    let env = HirTypeEnv::new()
        .with_local(7, Type::String)
        .with_function_return(3, Type::Boolean);

    assert_eq!(infer_expr_type(&Expr::LocalGet(7), &env), Type::String);

    let call = Expr::Call {
        callee: Box::new(Expr::FuncRef(3)),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    };
    assert_eq!(infer_expr_type(&call, &env), Type::Boolean);
}

#[test]
fn can_use_borrowed_type_facts_without_materializing_env() {
    struct BorrowedFacts<'a>(&'a [(LocalId, Type)]);

    impl HirTypeFacts for BorrowedFacts<'_> {
        fn local_type(&self, id: LocalId) -> Option<&Type> {
            self.0
                .iter()
                .find_map(|(local_id, ty)| (*local_id == id).then_some(ty))
        }

        fn global_type(&self, _id: GlobalId) -> Option<&Type> {
            None
        }

        fn function_return_type(&self, _id: FuncId) -> Option<&Type> {
            None
        }
    }

    let facts = BorrowedFacts(&[(9, Type::String)]);
    assert_eq!(infer_expr_type(&Expr::LocalGet(9), &facts), Type::String);
    assert_eq!(infer_expr_type(&Expr::LocalGet(10), &facts), Type::Any);
}

#[test]
fn can_use_local_type_maps_as_type_facts() {
    let mut facts = HashMap::new();
    facts.insert(11, Type::Boolean);

    assert_eq!(infer_expr_type(&Expr::LocalGet(11), &facts), Type::Boolean);
    assert_eq!(infer_expr_type(&Expr::GlobalGet(11), &facts), Type::Any);
}

#[test]
fn can_use_external_function_return_facts() {
    struct ExternFacts {
        returns: HashMap<String, Type>,
    }

    impl HirTypeFacts for ExternFacts {
        fn local_type(&self, _id: LocalId) -> Option<&Type> {
            None
        }

        fn global_type(&self, _id: GlobalId) -> Option<&Type> {
            None
        }

        fn function_return_type(&self, _id: FuncId) -> Option<&Type> {
            None
        }

        fn extern_function_return_type(&self, name: &str) -> Option<&Type> {
            self.returns.get(name)
        }
    }

    let facts = ExternFacts {
        returns: HashMap::from([("readName".to_string(), Type::String)]),
    };
    let call = Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "readName".to_string(),
            param_types: vec![],
            return_type: Type::Any,
        }),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    };

    assert_eq!(infer_expr_type(&call, &facts), Type::String);

    let any_fact_does_not_override_embedded_return = Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "maybeCount".to_string(),
            param_types: vec![],
            return_type: Type::Number,
        }),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    };
    let facts = ExternFacts {
        returns: HashMap::from([("maybeCount".to_string(), Type::Any)]),
    };

    assert_eq!(
        infer_expr_type(&any_fact_does_not_override_embedded_return, &facts),
        Type::Number
    );
}

#[test]
fn infer_refinable_expr_type_filters_non_value_results() {
    let env = empty_env();

    assert_eq!(infer_refinable_expr_type(&Expr::LocalGet(404), &env), None);
    assert_eq!(infer_refinable_expr_type(&Expr::Undefined, &env), None);
    assert_eq!(
        infer_refinable_expr_type(&Expr::ProcessExit(None), &env),
        None
    );
    assert_eq!(
        infer_refinable_expr_type(&Expr::SymbolKeyFor(Box::new(Expr::SymbolNew(None))), &env),
        None
    );
    assert_eq!(
        infer_refinable_expr_type(&Expr::String("x".to_string()), &env),
        Some(Type::String)
    );
}

#[test]
fn infers_object_and_array_shapes_conservatively() {
    let env = empty_env();
    let object = Expr::Object(vec![("answer".to_string(), Expr::Number(42.0))]);
    let get = Expr::PropertyGet {
        byte_offset: 0,
        object: Box::new(object),
        property: "answer".to_string(),
    };
    assert_eq!(infer_expr_type(&get, &env), Type::Number);

    let array = Expr::Array(vec![Expr::Number(1.0), Expr::String("x".to_string())]);
    assert_eq!(
        infer_expr_type(&array, &env),
        Type::Array(Box::new(Type::Any))
    );
}

#[test]
fn infers_array_higher_order_result_elements() {
    let env = HirTypeEnv::new().with_local(
        1,
        Type::Array(Box::new(Type::Array(Box::new(Type::Boolean)))),
    );

    assert_eq!(
        infer_expr_type(
            &Expr::ArrayMap {
                array: Box::new(Expr::Array(vec![Expr::Integer(1)])),
                callback: Box::new(closure_returning(Type::String)),
            },
            &env,
        ),
        Type::Array(Box::new(Type::String))
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayFlatMap {
                array: Box::new(Expr::Array(vec![Expr::Integer(1)])),
                callback: Box::new(closure_returning(Type::Array(Box::new(Type::Number)))),
            },
            &env,
        ),
        Type::Array(Box::new(Type::Number))
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayFlat {
                array: Box::new(Expr::LocalGet(1)),
            },
            &env,
        ),
        Type::Array(Box::new(Type::Boolean))
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayFromMapped {
                iterable: Box::new(Expr::Array(vec![Expr::Integer(1)])),
                map_fn: Box::new(closure_returning(Type::BigInt)),
                this_arg: None,
            },
            &env,
        ),
        Type::Array(Box::new(Type::BigInt))
    );
}

#[test]
fn infers_dynamic_passthrough_and_intrinsic_result_types() {
    let env = HirTypeEnv::new().with_local(1, Type::Array(Box::new(Type::String)));

    assert_eq!(
        infer_expr_type(
            &Expr::WithGet {
                object: Box::new(Expr::Object(vec![(
                    "name".to_string(),
                    Expr::String("x".to_string())
                )])),
                property: "name".to_string(),
                fallback: Box::new(Expr::String("fallback".to_string())),
            },
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::WithGet {
                object: Box::new(Expr::Object(vec![(
                    "name".to_string(),
                    Expr::String("x".to_string())
                )])),
                property: "name".to_string(),
                fallback: Box::new(Expr::Number(1.0)),
            },
            &env,
        ),
        Type::Any
    );
    assert_eq!(
        infer_expr_type(
            &Expr::JsSetProperty {
                object: Box::new(Expr::Object(vec![])),
                property_name: "value".to_string(),
                value: Box::new(Expr::BigInt("1".to_string())),
            },
            &env,
        ),
        Type::BigInt
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ReflectApply {
                func: Box::new(closure_returning(Type::Number)),
                this_arg: Box::new(Expr::Undefined),
                args: Box::new(Expr::Array(vec![])),
            },
            &env,
        ),
        Type::Number
    );
    assert_eq!(
        infer_expr_type(&Expr::SuperCall(vec![Expr::Integer(1)]), &env),
        Type::Void
    );
    assert_eq!(
        infer_expr_type(
            &Expr::SuperCallSpread(vec![CallArg::Spread(Expr::Array(vec![]))]),
            &env,
        ),
        Type::Void
    );
    assert_eq!(
        infer_expr_type(&Expr::NativeArenaAlloc(Box::new(Expr::Integer(64))), &env),
        Type::Named("NativeArenaOwner".to_string())
    );
    assert_eq!(
        infer_expr_type(
            &Expr::NativeArenaView {
                owner: Box::new(Expr::NativeArenaAlloc(Box::new(Expr::Integer(64)))),
                kind: 4,
                byte_offset: Box::new(Expr::Integer(0)),
                length: Box::new(Expr::Integer(8)),
            },
            &env,
        ),
        Type::Named("Int32Array".to_string())
    );
    let pod_view_type = Type::Generic {
        base: "PerryPodView".to_string(),
        type_args: vec![Type::Named("Point".to_string())],
    };
    assert_eq!(
        infer_expr_type(
            &Expr::NativePodView {
                owner: Box::new(Expr::NativeArenaAlloc(Box::new(Expr::Integer(64)))),
                byte_offset: Box::new(Expr::Integer(0)),
                count: Box::new(Expr::Integer(2)),
                view_type: Some(pod_view_type.clone()),
            },
            &env,
        ),
        pod_view_type
    );
}

#[test]
fn infers_arraylike_method_result_families() {
    let env = HirTypeEnv::new().with_local(1, Type::Array(Box::new(Type::String)));

    assert_eq!(
        infer_expr_type(
            &Expr::ArrayLikeMethod {
                method: "map".to_string(),
                receiver: Box::new(Expr::LocalGet(1)),
                args: vec![closure_returning(Type::Number)],
            },
            &env,
        ),
        Type::Array(Box::new(Type::Number))
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayLikeMethod {
                method: "filter".to_string(),
                receiver: Box::new(Expr::LocalGet(1)),
                args: vec![closure_returning(Type::Boolean)],
            },
            &env,
        ),
        Type::Array(Box::new(Type::String))
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayLikeMethod {
                method: "findIndex".to_string(),
                receiver: Box::new(Expr::LocalGet(1)),
                args: vec![closure_returning(Type::Boolean)],
            },
            &env,
        ),
        Type::Number
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayLikeMethod {
                method: "includes".to_string(),
                receiver: Box::new(Expr::LocalGet(1)),
                args: vec![Expr::String("x".to_string())],
            },
            &env,
        ),
        Type::Boolean
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayLikeMethod {
                method: "join".to_string(),
                receiver: Box::new(Expr::LocalGet(1)),
                args: Vec::new(),
            },
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayLikeMethod {
                method: "at".to_string(),
                receiver: Box::new(Expr::LocalGet(1)),
                args: vec![Expr::Integer(0)],
            },
            &env,
        ),
        Type::Union(vec![Type::String, Type::Void])
    );
}

#[test]
fn infers_passthrough_expression_types() {
    let env = HirTypeEnv::new()
        .with_local(1, Type::Named("Widget".to_string()))
        .with_local(2, Type::Array(Box::new(Type::String)));

    assert_eq!(
        infer_expr_type(
            &Expr::PrivateGuard {
                class_name: "Widget".to_string(),
                class_id: 0,
                field_name: "value".to_string(),
                kind: 0,
                op: 0,
                object: Box::new(Expr::LocalGet(1)),
            },
            &env,
        ),
        Type::Named("Widget".to_string())
    );
    assert_eq!(
        infer_expr_type(
            &Expr::Sequence(vec![Expr::Number(1.0), Expr::String("done".to_string())]),
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::StructuredClone {
                value: Box::new(Expr::LocalGet(2)),
                options: Box::new(Expr::Undefined),
            },
            &env,
        ),
        Type::Array(Box::new(Type::String))
    );
}

#[test]
fn infers_object_helper_runtime_values() {
    let env = HirTypeEnv::new().with_local(1, Type::Named("Widget".to_string()));

    assert_eq!(
        infer_expr_type(&Expr::ObjectFreeze(Box::new(Expr::LocalGet(1))), &env,),
        Type::Named("Widget".to_string())
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ObjectDefineProperty(
                Box::new(Expr::LocalGet(1)),
                Box::new(Expr::String("x".to_string())),
                Box::new(Expr::Object(vec![])),
            ),
            &env,
        ),
        Type::Named("Widget".to_string())
    );
    assert_eq!(
        infer_expr_type(&Expr::ObjectCreate(Box::new(Expr::Null), None), &env),
        Type::Object(ObjectType::default())
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ReflectGetMetadataKeys {
                target: Box::new(Expr::Object(vec![])),
                property_key: None,
            },
            &env,
        ),
        Type::Array(Box::new(Type::Any))
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ReflectGetOwnMetadataKeys {
                target: Box::new(Expr::Object(vec![])),
                property_key: Some(Box::new(Expr::String("x".to_string()))),
            },
            &env,
        ),
        Type::Array(Box::new(Type::Any))
    );
}

#[test]
fn seeds_environment_from_module_declarations() {
    let mut module = Module::new("test");
    module.init.push(Stmt::Let {
        id: 1,
        name: "value".to_string(),
        ty: Type::Number,
        mutable: false,
        init: Some(Expr::Number(1.0)),
    });

    let env = HirTypeEnv::from_module(&module);
    assert_eq!(env.local_type(1), Some(&Type::Number));
    assert_eq!(infer_expr_type(&Expr::LocalGet(1), &env), Type::Number);
}

#[test]
fn seeds_contextual_class_and_enum_facts_from_module() {
    let mut module = Module::new("test");
    module.enums.push(Enum {
        id: 1,
        name: "Color".to_string(),
        members: vec![
            EnumMember {
                name: "Red".to_string(),
                value: EnumValue::String("red".to_string()),
            },
            EnumMember {
                name: "Blue".to_string(),
                value: EnumValue::Number(2),
            },
        ],
        is_exported: false,
    });
    module.classes.push(Class {
        id: 1,
        name: "Widget".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
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
        static_fields: vec![ClassField {
            name: "count".to_string(),
            key_expr: None,
            ty: Type::Number,
            init: Some(Expr::Number(1.0)),
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
        static_methods: vec![Function {
            id: 2,
            name: "make".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Named("Widget".to_string()),
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
    });

    let env = HirTypeEnv::from_module(&module);
    assert_eq!(
        infer_expr_type(
            &Expr::EnumMember {
                enum_name: "Color".to_string(),
                member_name: "Red".to_string(),
            },
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::EnumMember {
                enum_name: "Color".to_string(),
                member_name: "Blue".to_string(),
            },
            &env,
        ),
        Type::Number
    );
    assert_eq!(
        infer_expr_type(
            &Expr::StaticFieldGet {
                class_name: "Widget".to_string(),
                field_name: "count".to_string(),
            },
            &env,
        ),
        Type::Number
    );
    assert_eq!(
        infer_expr_type(
            &Expr::StaticMethodCall {
                class_name: "Widget".to_string(),
                method_name: "make".to_string(),
                args: Vec::new(),
            },
            &env,
        ),
        Type::Named("Widget".to_string())
    );
}

#[test]
fn infers_named_class_and_interface_property_facts() {
    let mut module = Module::new("test");
    module.classes.push(Class {
        id: 1,
        name: "Base".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![class_field("label", Type::String)],
        constructor: None,
        methods: vec![function_decl(10, "score", Type::Number)],
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: vec![class_field("kind", Type::Boolean)],
        static_methods: vec![function_decl(11, "version", Type::String)],
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        is_nested: false,
        aliases: Vec::new(),
    });
    module.classes.push(Class {
        id: 2,
        name: "Child".to_string(),
        type_params: Vec::new(),
        extends: Some(1),
        extends_name: None,
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
        is_nested: false,
        aliases: Vec::new(),
    });
    module.interfaces.push(Interface {
        id: 1,
        name: "ParentShape".to_string(),
        type_params: Vec::new(),
        extends: Vec::new(),
        properties: vec![InterfaceProperty {
            name: "items".to_string(),
            ty: Type::Array(Box::new(Type::String)),
            optional: false,
            readonly: false,
        }],
        methods: vec![InterfaceMethod {
            name: "done".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Boolean,
        }],
        is_exported: false,
    });
    module.interfaces.push(Interface {
        id: 2,
        name: "ChildShape".to_string(),
        type_params: Vec::new(),
        extends: vec![Type::Named("ParentShape".to_string())],
        properties: Vec::new(),
        methods: Vec::new(),
        is_exported: false,
    });

    let env = HirTypeEnv::from_module(&module)
        .with_local(1, Type::Named("Child".to_string()))
        .with_local(2, Type::Named("ChildShape".to_string()));

    assert_eq!(
        infer_expr_type(
            &Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(1)),
                property: "label".to_string(),
            },
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::LocalGet(1)),
                    property: "score".to_string(),
                }),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            },
            &env,
        ),
        Type::Number
    );
    assert_eq!(
        infer_expr_type(
            &Expr::StaticFieldGet {
                class_name: "Child".to_string(),
                field_name: "kind".to_string(),
            },
            &env,
        ),
        Type::Boolean
    );
    assert_eq!(
        infer_expr_type(
            &Expr::StaticMethodCall {
                class_name: "Child".to_string(),
                method_name: "version".to_string(),
                args: Vec::new(),
            },
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(2)),
                property: "items".to_string(),
            },
            &env,
        ),
        Type::Array(Box::new(Type::String))
    );
    assert_eq!(
        infer_expr_type(
            &Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::LocalGet(2)),
                    property: "done".to_string(),
                }),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            },
            &env,
        ),
        Type::Boolean
    );
}

#[test]
fn infers_common_constructed_runtime_values() {
    let env = empty_env();

    assert_eq!(
        infer_expr_type(
            &Expr::New {
                class_name: "Widget".to_string(),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
                cap_args_appended: 0,
            },
            &env,
        ),
        Type::Named("Widget".to_string())
    );
    assert_eq!(
        infer_expr_type(
            &Expr::New {
                class_name: "Array".to_string(),
                args: vec![Expr::Integer(4)],
                type_args: vec![],
                byte_offset: 0,
                cap_args_appended: 0,
            },
            &env,
        ),
        Type::Array(Box::new(Type::Any))
    );
    assert_eq!(
        infer_expr_type(&Expr::DateNew(vec![]), &env),
        Type::Named("Date".to_string())
    );
    assert_eq!(
        infer_expr_type(
            &Expr::UrlGetSearchParams(Box::new(Expr::UrlNew {
                url: Box::new(Expr::String("https://example.com".to_string())),
                base: None,
            })),
            &env,
        ),
        Type::Named("URLSearchParams".to_string())
    );
    assert_eq!(
        infer_expr_type(&Expr::WeakRefNew(Box::new(Expr::Object(vec![]))), &env),
        Type::Named("WeakRef".to_string())
    );
    assert_eq!(
        infer_expr_type(
            &Expr::FinalizationRegistryNew(Box::new(Expr::Undefined)),
            &env,
        ),
        Type::Named("FinalizationRegistry".to_string())
    );
    assert_eq!(
        infer_expr_type(&Expr::MapNew, &env),
        Type::Generic {
            base: "Map".to_string(),
            type_args: Vec::new()
        }
    );
    assert_eq!(
        infer_expr_type(&Expr::SetNew, &env),
        Type::Generic {
            base: "Set".to_string(),
            type_args: Vec::new()
        }
    );
    assert_eq!(
        infer_expr_type(&Expr::Uint8ArrayNew(None), &env),
        Type::Named("Uint8Array".to_string())
    );
    assert_eq!(
        infer_expr_type(&Expr::CryptoRandomBytes(Box::new(Expr::Integer(8))), &env),
        Type::Named("Uint8Array".to_string())
    );
    assert_eq!(
        infer_expr_type(&Expr::TypedArrayNew { kind: 4, arg: None }, &env),
        Type::Named("Int32Array".to_string())
    );
    assert_eq!(
        infer_expr_type(
            &Expr::NewDynamic {
                callee: Box::new(Expr::LocalGet(404)),
                args: Vec::new(),
                byte_offset: 0,
            },
            &env,
        ),
        Type::Object(ObjectType::default())
    );
    assert_eq!(
        infer_expr_type(
            &Expr::NewDynamicSpread {
                callee: Box::new(Expr::LocalGet(404)),
                args: vec![CallArg::Spread(Expr::Array(vec![]))],
                byte_offset: 0,
            },
            &env,
        ),
        Type::Object(ObjectType::default())
    );
}

#[test]
fn infers_process_and_buffer_runtime_values() {
    let env = empty_env();

    assert_eq!(
        infer_expr_type(&Expr::ProcessHrtimeBigint, &env),
        Type::BigInt
    );
    assert_eq!(
        infer_expr_type(&Expr::ProcessHrtime(None), &env),
        Type::Tuple(vec![Type::Number, Type::Number])
    );
    assert_eq!(
        infer_expr_type(&Expr::ProcessArgv, &env),
        Type::Array(Box::new(Type::String))
    );
    assert_eq!(
        infer_expr_type(
            &Expr::BufferToString {
                buffer: Box::new(Expr::Uint8ArrayNew(None)),
                encoding: None,
            },
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::FsReadFileSync(Box::new(Expr::String("file.txt".to_string()))),
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(&Expr::PodLayoutSizeOf { ty: Type::Number }, &env,),
        Type::Number
    );
}

#[test]
fn infers_json_stringify_variants_as_strings() {
    let env = empty_env();

    assert_eq!(
        infer_expr_type(&Expr::JsonStringify(Box::new(Expr::Object(vec![]))), &env),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::JsonStringifyPretty {
                value: Box::new(Expr::Object(vec![])),
                replacer: None,
                space: Box::new(Expr::Integer(2)),
            },
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::JsonStringifyFull(
                Box::new(Expr::Object(vec![])),
                Box::new(Expr::Null),
                Box::new(Expr::Integer(2)),
            ),
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::JsonParseTyped {
                text: Box::new(Expr::String("{\"x\":1}".to_string())),
                ty: Type::Object(ObjectType::default()),
                ordered_keys: None,
            },
            &env,
        ),
        Type::Object(ObjectType::default())
    );
}

#[test]
fn infers_array_method_runtime_values() {
    let env = empty_env();

    assert_eq!(
        infer_expr_type(
            &Expr::ArraySlice {
                array: Box::new(Expr::Array(vec![])),
                start: Box::new(Expr::Integer(0)),
                end: None,
            },
            &env,
        ),
        Type::Array(Box::new(Type::Any))
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayJoin {
                array: Box::new(Expr::Array(vec![])),
                separator: None,
            },
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArraySort {
                array: Box::new(Expr::Array(vec![])),
                comparator: Box::new(Expr::Undefined),
            },
            &env,
        ),
        Type::Array(Box::new(Type::Any))
    );
    assert_eq!(
        infer_expr_type(&Expr::ArrayIsArray(Box::new(Expr::Array(vec![]))), &env),
        Type::Boolean
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayFindIndex {
                array: Box::new(Expr::Array(vec![])),
                callback: Box::new(Expr::Undefined),
            },
            &env,
        ),
        Type::Number
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayForEach {
                array: Box::new(Expr::Array(vec![])),
                callback: Box::new(Expr::Undefined),
            },
            &env,
        ),
        Type::Void
    );
}

#[test]
fn infers_array_methods_from_receiver_facts() {
    let env = HirTypeEnv::new().with_local(1, Type::Array(Box::new(Type::String)));

    assert_eq!(
        infer_expr_type(
            &Expr::ArraySlice {
                array: Box::new(Expr::LocalGet(1)),
                start: Box::new(Expr::Integer(0)),
                end: None,
            },
            &env,
        ),
        Type::Array(Box::new(Type::String))
    );
    assert_eq!(
        infer_expr_type(&Expr::ArrayValues(Box::new(Expr::LocalGet(1))), &env),
        Type::Array(Box::new(Type::String))
    );
    assert_eq!(
        infer_expr_type(&Expr::ArrayEntries(Box::new(Expr::LocalGet(1))), &env),
        Type::Array(Box::new(Type::Tuple(vec![Type::Number, Type::String])))
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayWith {
                array: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(0)),
                value: Box::new(Expr::Number(1.0)),
            },
            &env,
        ),
        Type::Array(Box::new(Type::Any))
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayFrom(Box::new(Expr::String("abc".to_string()))),
            &env,
        ),
        Type::Array(Box::new(Type::String))
    );
    assert_eq!(
        infer_expr_type(&Expr::ArrayPop(1), &env),
        Type::Union(vec![Type::String, Type::Void])
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayAt {
                array: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(0)),
            },
            &env,
        ),
        Type::Union(vec![Type::String, Type::Void])
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayFind {
                array: Box::new(Expr::LocalGet(1)),
                callback: Box::new(Expr::Undefined),
            },
            &env,
        ),
        Type::Union(vec![Type::String, Type::Void])
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ArrayReduce {
                array: Box::new(Expr::LocalGet(1)),
                callback: Box::new(closure_returning(Type::Number)),
                initial: Some(Box::new(Expr::Integer(0))),
            },
            &env,
        ),
        Type::Number
    );
}

#[test]
fn infers_class_prototype_and_super_meta_value_shapes() {
    let env = empty_env();

    assert!(matches!(
        infer_expr_type(&Expr::NativeModuleRef("node:fs".to_string()), &env),
        Type::Object(_)
    ));
    assert!(matches!(
        infer_expr_type(&Expr::ClassRef("Widget".to_string()), &env),
        Type::Function(_)
    ));
    assert!(matches!(
        infer_expr_type(
            &Expr::ClassExprFresh {
                template: "Widget".to_string(),
                named_statics: Vec::new(),
                symbol_statics: Vec::new(),
                captured_args: Vec::new(),
            },
            &env,
        ),
        Type::Object(_)
    ));
    assert!(matches!(
        infer_expr_type(&Expr::CurrentStepClosure, &env),
        Type::Function(_)
    ));
    assert!(matches!(
        infer_expr_type(&Expr::NewTarget, &env),
        Type::Union(ref variants)
            if variants.contains(&Type::Void)
                && variants.iter().any(|variant| matches!(variant, Type::Function(_)))
    ));
    assert!(matches!(
        infer_expr_type(
            &Expr::WeakRefDeref(Box::new(Expr::WeakRefNew(Box::new(Expr::Object(
                Vec::new()
            ))))),
            &env,
        ),
        Type::Union(ref variants)
            if variants.contains(&Type::Void)
                && variants.iter().any(|variant| matches!(variant, Type::Object(_)))
    ));
    assert!(matches!(
        infer_expr_type(
            &Expr::ProxyConstruct {
                proxy: Box::new(Expr::ProxyNew {
                    target: Box::new(Expr::Object(Vec::new())),
                    handler: Box::new(Expr::Object(Vec::new())),
                }),
                args: Vec::new(),
            },
            &env,
        ),
        Type::Object(_)
    ));
    assert!(matches!(
        infer_expr_type(
            &Expr::ReflectConstruct {
                target: Box::new(Expr::ClassRef("Widget".to_string())),
                args: Box::new(Expr::Array(Vec::new())),
                new_target: Box::new(Expr::Undefined),
            },
            &env,
        ),
        Type::Object(_)
    ));

    assert_eq!(
        infer_expr_type(
            &Expr::ClassStaticSymbolSet {
                class_name: "Widget".to_string(),
                key: Box::new(Expr::SymbolFor(Box::new(Expr::String("k".to_string())))),
                value: Box::new(Expr::String("value".to_string())),
            },
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::SuperPropertySet {
                parent_class_id: 0,
                parent_class_name: Some("Base".to_string()),
                key: Box::new(Expr::String("name".to_string())),
                value: Box::new(Expr::Bool(true)),
            },
            &env,
        ),
        Type::Boolean
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ObjectSuperPropertySet {
                home: Box::new(Expr::Object(Vec::new())),
                key: Box::new(Expr::String("name".to_string())),
                value: Box::new(Expr::BigInt("1".to_string())),
                receiver: Box::new(Expr::Object(Vec::new())),
            },
            &env,
        ),
        Type::BigInt
    );
    assert_eq!(
        infer_expr_type(
            &Expr::SetFunctionPrototype {
                func: Box::new(Expr::FuncRef(1)),
                proto: Box::new(Expr::String("proto".to_string())),
            },
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::RegisterPrototypeMethod {
                class_name: "Widget".to_string(),
                method_name: "run".to_string(),
                value: Box::new(Expr::Bool(false)),
            },
            &env,
        ),
        Type::Boolean
    );
    assert_eq!(
        infer_expr_type(
            &Expr::RegisterFunctionPrototypeMethod {
                func: Box::new(Expr::FuncRef(1)),
                method_name: "run".to_string(),
                value: Box::new(Expr::Number(1.0)),
            },
            &env,
        ),
        Type::Number
    );

    let prototype_method_ty = infer_expr_type(
        &Expr::GetFunctionPrototypeMethod {
            func: Box::new(Expr::FuncRef(1)),
            method_name: "run".to_string(),
        },
        &env,
    );
    assert!(matches!(
        prototype_method_ty,
        Type::Union(ref variants)
            if variants.contains(&Type::Void)
                && variants.iter().any(|variant| matches!(variant, Type::Function(_)))
    ));

    for expr in [
        Expr::RegisterClassParentDynamic {
            class_name: "Widget".to_string(),
            parent_expr: Box::new(Expr::ClassRef("Base".to_string())),
        },
        Expr::RegisterClassCaptures {
            class_name: "Widget".to_string(),
            captures: vec![Expr::String("capture".to_string())],
        },
        Expr::RegisterClassStaticSymbol {
            class_name: "Widget".to_string(),
            key_expr: Box::new(Expr::SymbolFor(Box::new(Expr::String("k".to_string())))),
            value_expr: Box::new(Expr::String("value".to_string())),
        },
        Expr::RegisterClassComputedMethod {
            class_name: "Widget".to_string(),
            key_expr: Box::new(Expr::String("run".to_string())),
            method_name: "run".to_string(),
            is_static: false,
            param_count: 0,
            has_rest: false,
        },
        Expr::RegisterClassComputedAccessor {
            class_name: "Widget".to_string(),
            key_expr: Box::new(Expr::String("value".to_string())),
            getter_name: Some("getValue".to_string()),
            setter_name: None,
            is_static: false,
        },
    ] {
        assert_eq!(infer_expr_type(&expr, &env), Type::Void);
    }
}

#[test]
fn infers_map_and_set_methods_from_generic_facts() {
    let env = HirTypeEnv::new()
        .with_local(
            1,
            Type::Generic {
                base: "Map".to_string(),
                type_args: vec![Type::String, Type::Number],
            },
        )
        .with_local(
            2,
            Type::Generic {
                base: "Set".to_string(),
                type_args: vec![Type::Boolean],
            },
        )
        .with_local(3, function_type(Type::String))
        .with_local(4, Type::Array(Box::new(Type::Number)));

    assert_eq!(
        infer_expr_type(
            &Expr::MapNewFromArray(Box::new(Expr::Array(vec![Expr::Array(vec![
                Expr::String("k".to_string()),
                Expr::Number(1.0),
            ])]))),
            &env,
        ),
        Type::Generic {
            base: "Map".to_string(),
            type_args: vec![Type::String, Type::Number]
        }
    );
    assert_eq!(
        infer_expr_type(
            &Expr::SetNewFromArray(Box::new(Expr::Array(vec![Expr::Bool(true)]))),
            &env,
        ),
        Type::Generic {
            base: "Set".to_string(),
            type_args: vec![Type::Boolean]
        }
    );
    assert_eq!(
        infer_expr_type(
            &Expr::MapGroupBy {
                items: Box::new(Expr::LocalGet(4)),
                key_fn: Box::new(Expr::LocalGet(3)),
            },
            &env,
        ),
        Type::Generic {
            base: "Map".to_string(),
            type_args: vec![Type::String, Type::Array(Box::new(Type::Number))]
        }
    );
    assert_eq!(
        infer_expr_type(
            &Expr::MapGet {
                map: Box::new(Expr::LocalGet(1)),
                key: Box::new(Expr::String("k".to_string())),
            },
            &env,
        ),
        Type::Number
    );
    assert_eq!(
        infer_expr_type(&Expr::MapKeys(Box::new(Expr::LocalGet(1))), &env),
        Type::Array(Box::new(Type::String))
    );
    assert_eq!(
        infer_expr_type(&Expr::MapEntries(Box::new(Expr::LocalGet(1))), &env),
        Type::Array(Box::new(Type::Tuple(vec![Type::String, Type::Number])))
    );
    assert_eq!(
        infer_expr_type(
            &Expr::SetAdd {
                set_id: 2,
                value: Box::new(Expr::Bool(true)),
            },
            &env,
        ),
        Type::Generic {
            base: "Set".to_string(),
            type_args: vec![Type::Boolean]
        }
    );
    assert_eq!(
        infer_expr_type(&Expr::SetValues(Box::new(Expr::LocalGet(2))), &env),
        Type::Array(Box::new(Type::Boolean))
    );
}

#[test]
fn infers_assignment_like_and_url_search_params_results() {
    let env = empty_env();

    assert_eq!(
        infer_expr_type(
            &Expr::WithSet {
                object: Box::new(Expr::Object(vec![])),
                property: "name".to_string(),
                value: Box::new(Expr::String("perry".to_string())),
                fallback: WithSetFallback::Ignore,
                strict: false,
            },
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::UrlSetHref {
                url: Box::new(Expr::UrlNew {
                    url: Box::new(Expr::String("https://example.com".to_string())),
                    base: None,
                }),
                value: Box::new(Expr::String("https://example.org".to_string())),
            },
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::UrlSearchParamsGet {
                params: Box::new(Expr::UrlSearchParamsNew(None)),
                name: Box::new(Expr::String("q".to_string())),
            },
            &env,
        ),
        Type::Union(vec![Type::String, Type::Null])
    );
    assert_eq!(
        infer_expr_type(
            &Expr::UrlSearchParamsSet {
                params: Box::new(Expr::UrlSearchParamsNew(None)),
                name: Box::new(Expr::String("q".to_string())),
                value: Box::new(Expr::String("perry".to_string())),
            },
            &env,
        ),
        Type::Void
    );
    assert_eq!(
        infer_expr_type(
            &Expr::UrlSearchParamsMissingArgs {
                params: Box::new(Expr::UrlSearchParamsNew(None)),
                args: vec![],
                name_and_value: true,
            },
            &env,
        ),
        Type::Never
    );
}

#[test]
fn infers_misc_collection_and_reflection_results() {
    let env = empty_env();

    assert_eq!(
        infer_expr_type(
            &Expr::TaggedTemplateStrings {
                site_id: 1,
                cooked: vec![Expr::String("hello".to_string())],
                raw: vec!["hello".to_string()],
            },
            &env,
        ),
        Type::Array(Box::new(Type::String))
    );
    assert_eq!(
        infer_expr_type(&Expr::TemplateRaw(Box::new(Expr::Array(vec![]))), &env),
        Type::Union(vec![Type::Array(Box::new(Type::String)), Type::Void])
    );
    assert_eq!(
        infer_expr_type(&Expr::EnvGet("HOME".to_string()), &env),
        Type::Union(vec![Type::String, Type::Void])
    );
    assert_eq!(
        infer_expr_type(
            &Expr::GetIterator(Box::new(Expr::Array(vec![Expr::String(
                "item".to_string()
            )]))),
            &env,
        ),
        Type::Object(ObjectType::default())
    );
    assert_eq!(
        infer_expr_type(
            &Expr::StringMatchAll {
                string: Box::new(Expr::String("a".to_string())),
                regex: Box::new(Expr::RegExp {
                    pattern: "a".to_string(),
                    flags: "g".to_string(),
                }),
            },
            &env,
        ),
        Type::Object(ObjectType::default())
    );
    assert_eq!(
        infer_expr_type(&Expr::RegExpExecGroups, &env),
        Type::Union(vec![Type::Object(ObjectType::default()), Type::Void])
    );
    assert_eq!(
        infer_expr_type(&Expr::ReflectOwnKeys(Box::new(Expr::Object(vec![]))), &env),
        Type::Array(Box::new(Type::Union(vec![Type::String, Type::Symbol])))
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ObjectGetPrototypeOf(Box::new(Expr::Object(vec![]))),
            &env,
        ),
        Type::Union(vec![Type::Object(ObjectType::default()), Type::Null])
    );
    assert_eq!(
        infer_expr_type(
            &Expr::ReflectGetOwnPropertyDescriptor {
                target: Box::new(Expr::Object(vec![])),
                key: Box::new(Expr::String("x".to_string())),
            },
            &env,
        ),
        Type::Union(vec![Type::Object(ObjectType::default()), Type::Void])
    );
}

#[test]
fn path_to_namespaced_path_requires_string_input() {
    let env = HirTypeEnv::new()
        .with_local(1, Type::String)
        .with_local(2, Type::Number);

    assert_eq!(
        infer_expr_type(
            &Expr::PathToNamespacedPath(Box::new(Expr::LocalGet(1))),
            &env
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::PathToNamespacedPath(Box::new(Expr::LocalGet(2))),
            &env
        ),
        Type::Any
    );
}

#[test]
fn mixed_bigint_number_arithmetic_is_not_bigint() {
    let env = empty_env();
    let mixed = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::BigInt("1".to_string())),
        right: Box::new(Expr::Number(2.0)),
    };
    // `1n + 2` throws a TypeError at runtime, so it is not a BigInt.
    assert_eq!(infer_expr_type(&mixed, &env), Type::Any);

    let both = Expr::Binary {
        op: BinaryOp::Mul,
        left: Box::new(Expr::BigInt("2".to_string())),
        right: Box::new(Expr::BigInt("3".to_string())),
    };
    assert_eq!(infer_expr_type(&both, &env), Type::BigInt);
}

#[test]
fn logical_and_or_unions_both_operands() {
    let env = empty_env();
    // `0 && "x"` evaluates to `0` (a number), so the result is number | string,
    // not just the right operand's type.
    let expr = Expr::Logical {
        op: LogicalOp::And,
        left: Box::new(Expr::Number(0.0)),
        right: Box::new(Expr::String("x".to_string())),
    };
    assert_eq!(
        infer_expr_type(&expr, &env),
        Type::Union(vec![Type::Number, Type::String])
    );

    // Equal operand types collapse rather than forming a redundant union.
    let same = Expr::Logical {
        op: LogicalOp::Or,
        left: Box::new(Expr::String("a".to_string())),
        right: Box::new(Expr::String("b".to_string())),
    };
    assert_eq!(infer_expr_type(&same, &env), Type::String);
}

#[test]
fn resolves_this_and_super_in_class_context() {
    let mut module = Module::new("test");
    module.classes.push(Class {
        id: 1,
        name: "Base".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![class_field("label", Type::String)],
        constructor: None,
        methods: vec![function_decl(10, "score", Type::Number)],
        getters: Vec::new(),
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
    });
    module.classes.push(Class {
        id: 2,
        name: "Child".to_string(),
        type_params: Vec::new(),
        extends: Some(1),
        extends_name: None,
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
        is_nested: false,
        aliases: Vec::new(),
    });

    let mut env = HirTypeEnv::from_module(&module);
    let this_label = Expr::PropertyGet {
        byte_offset: 0,
        object: Box::new(Expr::This),
        property: "label".to_string(),
    };

    // No class context → `this.label` stays conservative.
    assert_eq!(infer_expr_type(&this_label, &env), Type::Any);

    // Inside Child's body, `this`/`super` resolve through the chain to Base.
    env.set_current_class(Some("Child".to_string()));
    assert_eq!(infer_expr_type(&this_label, &env), Type::String);
    assert_eq!(
        infer_expr_type(
            &Expr::SuperPropertyGet {
                property: "label".to_string()
            },
            &env,
        ),
        Type::String
    );
    assert_eq!(
        infer_expr_type(
            &Expr::SuperMethodCall {
                method: "score".to_string(),
                args: Vec::new(),
            },
            &env,
        ),
        Type::Number
    );
}
