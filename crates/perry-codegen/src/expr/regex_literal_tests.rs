//! Literal construction and factory calls use ordinary RegExp objects and method sites.

use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, ModuleInitKind, Param, Stmt};

fn function(
    id: u32,
    name: &str,
    params: Vec<Param>,
    body: Vec<Stmt>,
    return_type: Type,
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

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::String,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn call(callee: Expr, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(callee),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

fn property(object: Expr, property: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(object),
        property: property.to_string(),
        byte_offset: 0,
    }
}

fn compile(functions: Vec<Function>) -> String {
    let mut module = Module::new("regex_site_test.ts");
    module.functions = functions;
    module.init_kind = ModuleInitKind::Eager;
    String::from_utf8(
        crate::compile_module(&module, super::class_field_barrier_tests::ir_opts())
            .expect("regex site fixture compiles"),
    )
    .expect("LLVM IR is UTF-8")
}

fn assert_generic(ir: &str) {
    assert!(!ir.contains("js_regexp_site_"), "{ir}");
    assert!(!ir.contains("js_regexp_new_factory"), "{ir}");
    assert!(ir.contains("call double @js_method_site_miss("), "{ir}");
}

#[test]
fn direct_literal_test_constructs_data_site_and_uses_generic_method_call() {
    let ir = compile(vec![function(
        1,
        "direct",
        vec![param(10, "s")],
        vec![Stmt::Return(Some(call(
            property(
                Expr::RegExp {
                    pattern: "x".into(),
                    flags: "g".into(),
                },
                "test",
            ),
            vec![Expr::LocalGet(10)],
        )))],
        Type::Boolean,
    )]);
    assert!(ir.contains("call i64 @js_regexp_literal("), "{ir}");
    assert!(ir.contains("private global i64 0"), "{ir}");
    assert_generic(&ir);
}

#[test]
fn factory_test_uses_generic_method_call() {
    let factory = function(
        1,
        "factory",
        Vec::new(),
        vec![Stmt::Return(Some(Expr::RegExp {
            pattern: "x".into(),
            flags: "g".into(),
        }))],
        Type::Named("RegExp".into()),
    );
    let caller = function(
        2,
        "caller",
        vec![param(30, "s")],
        vec![Stmt::Return(Some(call(
            property(call(Expr::FuncRef(1), Vec::new()), "test"),
            vec![Expr::LocalGet(30)],
        )))],
        Type::Boolean,
    );
    let ir = compile(vec![factory, caller]);
    assert!(ir.contains("call i64 @js_regexp_literal("), "{ir}");
    assert_generic(&ir);
}

#[test]
fn ordinary_factory_test_uses_generic_method_call() {
    let ir = compile(vec![function(
        1,
        "member",
        Vec::new(),
        vec![Stmt::Return(Some(call(
            property(
                call(property(Expr::Undefined, "default"), Vec::new()),
                "test",
            ),
            vec![Expr::String("x".into())],
        )))],
        Type::Any,
    )]);
    assert_generic(&ir);
}

#[test]
fn typed_local_test_uses_generic_method_call() {
    let mut receiver = param(10, "re");
    receiver.ty = Type::Named("RegExp".into());
    let ir = compile(vec![function(
        1,
        "typed",
        vec![receiver, param(11, "s")],
        vec![Stmt::Return(Some(call(
            property(Expr::LocalGet(10), "test"),
            vec![Expr::LocalGet(11)],
        )))],
        Type::Boolean,
    )]);
    assert_generic(&ir);
    assert!(!ir.contains("call i32 @js_regexp_test("), "{ir}");
}
