use super::*;
use perry_types::TypeParam;

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
        }],
        return_type: Type::TypeVar("T".to_string()),
        body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
        is_async: false,
        is_generator: false,
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
        }],
        return_type: Type::TypeVar("T".to_string()),
        body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
        is_async: false,
        is_generator: false,
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
        }],
        return_type: Type::TypeVar("T".to_string()),
        body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
        is_async: false,
        is_generator: false,
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
        }],
        return_type: Type::TypeVar("T".to_string()),
        body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
        is_async: false,
        is_generator: false,
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
