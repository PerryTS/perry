use super::*;
use crate::types::TypeParam;

#[test]
fn test_mangle_type() {
    assert_eq!(mangle_type(&Type::Number), "num");
    assert_eq!(mangle_type(&Type::String), "str");
    assert_eq!(mangle_type(&Type::Array(Box::new(Type::Number))), "arr_num");
}

#[test]
fn test_generate_specialized_name() {
    assert_eq!(
        generate_specialized_name("identity", &[Type::Number]),
        "identity$num"
    );
    assert_eq!(
        generate_specialized_name("pair", &[Type::Number, Type::String]),
        "pair$num_str"
    );
}

#[test]
fn test_substitute_type() {
    let mut subs = HashMap::new();
    subs.insert("T".to_string(), Type::Number);

    assert_eq!(
        substitute_type(&Type::TypeVar("T".to_string()), &subs),
        Type::Number
    );
    assert_eq!(
        substitute_type(
            &Type::Array(Box::new(Type::TypeVar("T".to_string()))),
            &subs
        ),
        Type::Array(Box::new(Type::Number))
    );
}

#[test]
fn test_monomorphize_substitutes_pod_layout_type_vars() {
    let type_var_t = Type::TypeVar("T".to_string());
    let packet_ty = Type::Named("Packet".to_string());
    let layout_func = Function {
        id: 1,
        name: "layout".to_string(),
        type_params: vec![TypeParam {
            name: "T".to_string(),
            constraint: Some(Box::new(Type::Generic {
                base: "PerryPod".to_string(),
                type_args: vec![Type::Any],
            })),
            default: None,
        }],
        params: vec![],
        return_type: Type::Number,
        body: vec![
            Stmt::Expr(Expr::PodLayoutSizeOf {
                ty: type_var_t.clone(),
            }),
            Stmt::Expr(Expr::PodLayoutAlignOf {
                ty: type_var_t.clone(),
            }),
            Stmt::Return(Some(Expr::PodLayoutOffsetOf {
                ty: type_var_t,
                field_path: vec!["payload".to_string()],
            })),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
    };

    let mut module = Module::new("test");
    module.functions.push(layout_func);
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![],
        type_args: vec![packet_ty.clone()],
        byte_offset: 0,
    }));

    monomorphize_module(&mut module);

    let specialized = module
        .functions
        .iter()
        .find(|f| f.name == "layout$Packet")
        .expect("Specialized function layout$Packet should exist");

    assert!(matches!(
        &specialized.body[0],
        Stmt::Expr(Expr::PodLayoutSizeOf { ty }) if ty == &packet_ty
    ));
    assert!(matches!(
        &specialized.body[1],
        Stmt::Expr(Expr::PodLayoutAlignOf { ty }) if ty == &packet_ty
    ));
    assert!(matches!(
        &specialized.body[2],
        Stmt::Return(Some(Expr::PodLayoutOffsetOf { ty, field_path }))
            if ty == &packet_ty && field_path == &vec!["payload".to_string()]
    ));
}

#[test]
fn test_monomorphize_substitutes_native_pod_view_type_vars() {
    let packet_ty = Type::Named("Packet".to_string());
    let generic_view_ty = Type::Generic {
        base: "PerryPodView".to_string(),
        type_args: vec![Type::TypeVar("T".to_string())],
    };
    let concrete_view_ty = Type::Generic {
        base: "PerryPodView".to_string(),
        type_args: vec![packet_ty.clone()],
    };
    let view_func = Function {
        id: 1,
        name: "view".to_string(),
        type_params: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        params: vec![Param {
            id: 0,
            name: "arena".to_string(),
            ty: Type::Named("NativeArena".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: generic_view_ty.clone(),
        body: vec![Stmt::Return(Some(Expr::NativePodView {
            owner: Box::new(Expr::LocalGet(0)),
            byte_offset: Box::new(Expr::Integer(0)),
            count: Box::new(Expr::Integer(1)),
            view_type: Some(generic_view_ty),
        }))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
    };

    let mut module = Module::new("test");
    module.functions.push(view_func);
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::NativeArenaAlloc(Box::new(Expr::Integer(64)))],
        type_args: vec![packet_ty],
        byte_offset: 0,
    }));

    monomorphize_module(&mut module);

    let specialized = module
        .functions
        .iter()
        .find(|f| f.name == "view$Packet")
        .expect("Specialized function view$Packet should exist");

    assert_eq!(specialized.return_type, concrete_view_ty);
    assert!(matches!(
        &specialized.body[0],
        Stmt::Return(Some(Expr::NativePodView { view_type, .. }))
            if view_type.as_ref() == Some(&concrete_view_ty)
    ));
}

#[test]
fn test_monomorphize_generic_function() {
    // Create a generic identity function: function identity<T>(x: T): T { return x; }
    let identity_func = Function {
        id: 1,
        name: "identity".to_string(),
        type_params: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        params: vec![Param {
            id: 0,
            name: "x".to_string(),
            ty: Type::TypeVar("T".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::TypeVar("T".to_string()),
        body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
    };

    // Create a module with the generic function and a call to it with type args
    let mut module = Module::new("test");
    module.functions.push(identity_func);

    // Add init code that calls identity<number>(42)
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Number(42.0)],
        type_args: vec![Type::Number],
        byte_offset: 0,
    }));

    // Run monomorphization
    monomorphize_module(&mut module);

    // Verify that a specialized function was created
    assert_eq!(
        module.functions.len(),
        2,
        "Should have original + specialized function"
    );

    // Find the specialized function
    let specialized = module
        .functions
        .iter()
        .find(|f| f.name == "identity$num")
        .expect("Specialized function identity$num should exist");

    // Verify the specialized function has correct types
    assert!(
        specialized.type_params.is_empty(),
        "Specialized function should have no type params"
    );
    assert_eq!(
        specialized.params[0].ty,
        Type::Number,
        "Param should be Number"
    );
    assert_eq!(
        specialized.return_type,
        Type::Number,
        "Return type should be Number"
    );
}

#[test]
fn test_monomorphize_updates_call_sites() {
    // Create a generic function
    let identity_func = Function {
        id: 1,
        name: "identity".to_string(),
        type_params: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        params: vec![Param {
            id: 0,
            name: "x".to_string(),
            ty: Type::TypeVar("T".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::TypeVar("T".to_string()),
        body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
    };

    let mut module = Module::new("test");
    module.functions.push(identity_func);

    // Add call to identity<string>("hello")
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::String("hello".to_string())],
        type_args: vec![Type::String],
        byte_offset: 0,
    }));

    // Run monomorphization
    monomorphize_module(&mut module);

    // Check that the call site was updated to use the specialized function
    if let Stmt::Expr(Expr::Call {
        callee, type_args, ..
    }) = &module.init[0]
    {
        if let Expr::FuncRef(func_id) = callee.as_ref() {
            // The call should now reference the specialized function (id >= 1000)
            assert!(
                *func_id >= 1000,
                "Call should reference specialized function, got id {}",
                func_id
            );
            // Type args should be cleared
            assert!(
                type_args.is_empty(),
                "Type args should be cleared after monomorphization"
            );
        } else {
            panic!("Expected FuncRef callee");
        }
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_monomorphize_updates_native_memory_copy_operand_generic_calls() {
    let packet_ty = Type::Named("Packet".to_string());
    let mut module = Module::new("test");
    module.functions.push(generic_view_function(1, "makeView"));
    module.functions.push(generic_view_function(2, "other"));

    module.init.push(Stmt::Expr(Expr::NativeMemoryCopy {
        dst: Box::new(Expr::Call {
            callee: Box::new(Expr::FuncRef(1)),
            args: vec![],
            type_args: vec![packet_ty.clone()],
            byte_offset: 0,
        }),
        src: Box::new(Expr::Call {
            callee: Box::new(Expr::FuncRef(2)),
            args: vec![],
            type_args: vec![packet_ty],
            byte_offset: 0,
        }),
    }));

    monomorphize_module(&mut module);

    assert!(module.functions.iter().any(|f| f.name == "makeView$Packet"));
    assert!(module.functions.iter().any(|f| f.name == "other$Packet"));

    let Stmt::Expr(Expr::NativeMemoryCopy { dst, src }) = &module.init[0] else {
        panic!("Expected NativeMemory.copy expression");
    };

    assert_specialized_call(dst, &module, "makeView$Packet");
    assert_specialized_call(src, &module, "other$Packet");
}

#[test]
fn test_monomorphize_updates_native_memory_fill_u32_operand_generic_calls() {
    let packet_ty = Type::Named("Packet".to_string());
    let mut module = Module::new("test");
    module.functions.push(generic_view_function(1, "makeView"));
    module.functions.push(generic_number_function(2, "value"));

    module.init.push(Stmt::Expr(Expr::NativeMemoryFillU32 {
        view: Box::new(Expr::Call {
            callee: Box::new(Expr::FuncRef(1)),
            args: vec![],
            type_args: vec![packet_ty.clone()],
            byte_offset: 0,
        }),
        value: Box::new(Expr::Call {
            callee: Box::new(Expr::FuncRef(2)),
            args: vec![],
            type_args: vec![packet_ty],
            byte_offset: 0,
        }),
    }));

    monomorphize_module(&mut module);

    assert!(module.functions.iter().any(|f| f.name == "makeView$Packet"));
    assert!(module.functions.iter().any(|f| f.name == "value$Packet"));

    let Stmt::Expr(Expr::NativeMemoryFillU32 { view, value }) = &module.init[0] else {
        panic!("Expected NativeMemory.fillU32 expression");
    };

    assert_specialized_call(view, &module, "makeView$Packet");
    assert_specialized_call(value, &module, "value$Packet");
}

#[test]
fn test_monomorphize_updates_native_arena_alloc_size_generic_call() {
    let packet_ty = Type::Named("Packet".to_string());
    let mut module = Module::new("test");
    module
        .functions
        .push(generic_number_function(1, "byteLength"));

    module
        .init
        .push(Stmt::Expr(Expr::NativeArenaAlloc(Box::new(generic_call(
            1, &packet_ty,
        )))));

    monomorphize_module(&mut module);

    assert!(module
        .functions
        .iter()
        .any(|f| f.name == "byteLength$Packet"));

    let Stmt::Expr(Expr::NativeArenaAlloc(size)) = &module.init[0] else {
        panic!("Expected NativeArena.alloc expression");
    };
    assert_specialized_call(size, &module, "byteLength$Packet");
}

#[test]
fn test_monomorphize_updates_native_arena_view_operand_generic_calls() {
    let packet_ty = Type::Named("Packet".to_string());
    let mut module = Module::new("test");
    module
        .functions
        .push(generic_arena_function(1, "makeArena"));
    module.functions.push(generic_number_function(2, "offset"));
    module.functions.push(generic_number_function(3, "length"));

    module.init.push(Stmt::Expr(Expr::NativeArenaView {
        owner: Box::new(generic_call(1, &packet_ty)),
        kind: 1,
        byte_offset: Box::new(generic_call(2, &packet_ty)),
        length: Box::new(generic_call(3, &packet_ty)),
    }));

    monomorphize_module(&mut module);

    assert!(module
        .functions
        .iter()
        .any(|f| f.name == "makeArena$Packet"));
    assert!(module.functions.iter().any(|f| f.name == "offset$Packet"));
    assert!(module.functions.iter().any(|f| f.name == "length$Packet"));

    let Stmt::Expr(Expr::NativeArenaView {
        owner,
        byte_offset,
        length,
        ..
    }) = &module.init[0]
    else {
        panic!("Expected NativeArena.view expression");
    };

    assert_specialized_call(owner, &module, "makeArena$Packet");
    assert_specialized_call(byte_offset, &module, "offset$Packet");
    assert_specialized_call(length, &module, "length$Packet");
}

#[test]
fn test_monomorphize_updates_native_pod_view_operand_generic_calls() {
    let packet_ty = Type::Named("Packet".to_string());
    let mut module = Module::new("test");
    module
        .functions
        .push(generic_arena_function(1, "makeArena"));
    module.functions.push(generic_number_function(2, "offset"));
    module.functions.push(generic_number_function(3, "count"));

    module.init.push(Stmt::Expr(Expr::NativePodView {
        owner: Box::new(generic_call(1, &packet_ty)),
        byte_offset: Box::new(generic_call(2, &packet_ty)),
        count: Box::new(generic_call(3, &packet_ty)),
        view_type: Some(Type::Generic {
            base: "PerryPodView".to_string(),
            type_args: vec![packet_ty.clone()],
        }),
    }));

    monomorphize_module(&mut module);

    assert!(module
        .functions
        .iter()
        .any(|f| f.name == "makeArena$Packet"));
    assert!(module.functions.iter().any(|f| f.name == "offset$Packet"));
    assert!(module.functions.iter().any(|f| f.name == "count$Packet"));

    let Stmt::Expr(Expr::NativePodView {
        owner,
        byte_offset,
        count,
        ..
    }) = &module.init[0]
    else {
        panic!("Expected NativeArena.podView expression");
    };

    assert_specialized_call(owner, &module, "makeArena$Packet");
    assert_specialized_call(byte_offset, &module, "offset$Packet");
    assert_specialized_call(count, &module, "count$Packet");
}

#[test]
fn test_monomorphize_updates_native_arena_dispose_owner_generic_call() {
    let packet_ty = Type::Named("Packet".to_string());
    let mut module = Module::new("test");
    module
        .functions
        .push(generic_arena_function(1, "makeArena"));

    module
        .init
        .push(Stmt::Expr(Expr::NativeArenaDispose(Box::new(
            generic_call(1, &packet_ty),
        ))));

    monomorphize_module(&mut module);

    assert!(module
        .functions
        .iter()
        .any(|f| f.name == "makeArena$Packet"));

    let Stmt::Expr(Expr::NativeArenaDispose(owner)) = &module.init[0] else {
        panic!("Expected NativeArena.dispose expression");
    };
    assert_specialized_call(owner, &module, "makeArena$Packet");
}

#[test]
fn test_type_inference_from_arguments() {
    // Create a generic identity function: function identity<T>(x: T): T { return x; }
    let identity_func = Function {
        id: 1,
        name: "identity".to_string(),
        type_params: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        params: vec![Param {
            id: 0,
            name: "x".to_string(),
            ty: Type::TypeVar("T".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::TypeVar("T".to_string()),
        body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
    };

    let mut module = Module::new("test");
    module.functions.push(identity_func);

    // Add call to identity(42) WITHOUT explicit type args - should infer number
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Number(42.0)],
        type_args: vec![], // Empty - should be inferred!
        byte_offset: 0,
    }));

    // Run monomorphization
    monomorphize_module(&mut module);

    // Verify that a specialized function was created even without explicit type args
    assert_eq!(
        module.functions.len(),
        2,
        "Should have original + specialized function"
    );

    // Find the specialized function
    let specialized = module
        .functions
        .iter()
        .find(|f| f.name == "identity$num")
        .expect("Specialized function identity$num should exist (inferred from Number argument)");

    // Verify the specialized function has correct types
    assert!(
        specialized.type_params.is_empty(),
        "Specialized function should have no type params"
    );
    assert_eq!(
        specialized.params[0].ty,
        Type::Number,
        "Param should be Number"
    );
    assert_eq!(
        specialized.return_type,
        Type::Number,
        "Return type should be Number"
    );

    // Check that the call site was updated to use the specialized function
    if let Stmt::Expr(Expr::Call {
        callee, type_args, ..
    }) = &module.init[0]
    {
        if let Expr::FuncRef(func_id) = callee.as_ref() {
            // The call should now reference the specialized function (id >= 1000)
            assert!(
                *func_id >= 1000,
                "Call should reference specialized function, got id {}",
                func_id
            );
            // Type args should remain empty
            assert!(type_args.is_empty(), "Type args should be empty");
        } else {
            panic!("Expected FuncRef callee");
        }
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_type_inference_string() {
    // Create a generic identity function
    let identity_func = Function {
        id: 1,
        name: "identity".to_string(),
        type_params: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        params: vec![Param {
            id: 0,
            name: "x".to_string(),
            ty: Type::TypeVar("T".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::TypeVar("T".to_string()),
        body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
    };

    let mut module = Module::new("test");
    module.functions.push(identity_func);

    // Add call to identity("hello") WITHOUT explicit type args - should infer string
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::String("hello".to_string())],
        type_args: vec![], // Empty - should be inferred!
        byte_offset: 0,
    }));

    // Run monomorphization
    monomorphize_module(&mut module);

    // Find the specialized function
    let specialized = module
        .functions
        .iter()
        .find(|f| f.name == "identity$str")
        .expect("Specialized function identity$str should exist (inferred from String argument)");

    // Verify the specialized function has correct types
    assert_eq!(
        specialized.params[0].ty,
        Type::String,
        "Param should be String"
    );
    assert_eq!(
        specialized.return_type,
        Type::String,
        "Return type should be String"
    );
}

#[test]
fn test_type_inference_rest_type_var_binds_tuple() {
    let collect_func = Function {
        id: 1,
        name: "collect".to_string(),
        type_params: vec![TypeParam {
            name: "Params".to_string(),
            constraint: Some(Box::new(Type::Generic {
                base: "ReadonlyArray".to_string(),
                type_args: vec![Type::String],
            })),
            default: None,
        }],
        params: vec![Param {
            id: 0,
            name: "params".to_string(),
            ty: Type::TypeVar("Params".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: true,
            arguments_object: None,
        }],
        return_type: Type::TypeVar("Params".to_string()),
        body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
    };

    let mut module = Module::new("test");
    module.functions.push(collect_func);
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![
            Expr::String("a".to_string()),
            Expr::String("b".to_string()),
            Expr::String("c".to_string()),
        ],
        type_args: vec![],
        byte_offset: 0,
    }));

    monomorphize_module(&mut module);

    let specialized = module
        .functions
        .iter()
        .find(|f| f.name == "collect$tup_str_str_str")
        .expect("rest type variable should specialize as the trailing tuple");

    assert!(specialized.params[0].is_rest);
    assert_eq!(
        specialized.params[0].ty,
        Type::Tuple(vec![Type::String, Type::String, Type::String])
    );
}

#[test]
fn test_type_inference_rest_array_binds_element_type() {
    let collect_func = Function {
        id: 1,
        name: "collect".to_string(),
        type_params: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        params: vec![Param {
            id: 0,
            name: "items".to_string(),
            ty: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
            default: None,
            decorators: Vec::new(),
            is_rest: true,
            arguments_object: None,
        }],
        return_type: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
        body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
    };

    let mut module = Module::new("test");
    module.functions.push(collect_func);
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::String("a".to_string()), Expr::String("b".to_string())],
        type_args: vec![],
        byte_offset: 0,
    }));

    monomorphize_module(&mut module);

    let specialized = module
        .functions
        .iter()
        .find(|f| f.name == "collect$str")
        .expect("rest array should specialize by element type");

    assert!(specialized.params[0].is_rest);
    assert_eq!(
        specialized.params[0].ty,
        Type::Array(Box::new(Type::String))
    );
}

fn generic_view_function(id: FuncId, name: &str) -> Function {
    let view_ty = Type::Generic {
        base: "PerryPodView".to_string(),
        type_args: vec![Type::TypeVar("T".to_string())],
    };

    Function {
        id,
        name: name.to_string(),
        type_params: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        params: vec![],
        return_type: view_ty.clone(),
        body: vec![Stmt::Return(Some(Expr::NativePodView {
            owner: Box::new(Expr::NativeArenaAlloc(Box::new(Expr::Integer(64)))),
            byte_offset: Box::new(Expr::Integer(0)),
            count: Box::new(Expr::Integer(1)),
            view_type: Some(view_ty),
        }))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
    }
}

fn generic_number_function(id: FuncId, name: &str) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        params: vec![],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::Number(1.0)))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
    }
}

fn generic_arena_function(id: FuncId, name: &str) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        params: vec![],
        return_type: Type::Named("NativeArena".to_string()),
        body: vec![Stmt::Return(Some(Expr::NativeArenaAlloc(Box::new(
            Expr::Integer(64),
        ))))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
    }
}

fn generic_call(func_id: FuncId, ty: &Type) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::FuncRef(func_id)),
        args: vec![],
        type_args: vec![ty.clone()],
        byte_offset: 0,
    }
}

fn assert_specialized_call(expr: &Expr, module: &Module, expected_name: &str) {
    let Expr::Call {
        callee, type_args, ..
    } = expr
    else {
        panic!("Expected Call expression");
    };

    let Expr::FuncRef(func_id) = callee.as_ref() else {
        panic!("Expected FuncRef callee");
    };

    let func = module
        .functions
        .iter()
        .find(|f| f.id == *func_id)
        .expect("Call should reference an existing function");

    assert_eq!(func.name, expected_name);
    assert!(
        type_args.is_empty(),
        "Type args should be cleared after monomorphization"
    );
}
