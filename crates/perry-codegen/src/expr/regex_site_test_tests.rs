//! Allocation-free regex `.test` site lowering.  These are IR-shape tests so
//! deleting a specialization while leaving the runtime helpers behind fails.

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

#[test]
fn direct_literal_test_uses_the_site_header_and_post_get_dispatch() {
    let body = vec![Stmt::Return(Some(Expr::RegExpTest {
        regex: Box::new(Expr::RegExp {
            pattern: "x".to_string(),
            flags: "g".to_string(),
        }),
        string: Box::new(Expr::LocalGet(10)),
    }))];
    let ir = compile(vec![function(
        1,
        "direct",
        vec![param(10, "s")],
        body,
        Type::Boolean,
    )]);
    assert!(ir.contains("call i64 @js_regexp_site_test_new("), "{ir}");
    assert!(
        ir.contains("call double @js_regexp_site_test_get_method("),
        "{ir}"
    );
    assert!(
        ir.contains("call double @js_regexp_site_test_dispatch("),
        "{ir}"
    );
}

#[test]
fn escaping_literal_is_not_transformed_and_keeps_one_stateful_receiver() {
    let body = vec![
        Stmt::Let {
            id: 20,
            name: "r".to_string(),
            ty: Type::Named("RegExp".to_string()),
            mutable: false,
            init: Some(Expr::RegExp {
                pattern: "x".to_string(),
                flags: "g".to_string(),
            }),
        },
        Stmt::Expr(Expr::RegExpTest {
            regex: Box::new(Expr::LocalGet(20)),
            string: Box::new(Expr::LocalGet(21)),
        }),
        Stmt::Return(Some(Expr::RegExpTest {
            regex: Box::new(Expr::LocalGet(20)),
            string: Box::new(Expr::LocalGet(22)),
        })),
    ];
    let ir = compile(vec![function(
        1,
        "escaping",
        vec![param(21, "a"), param(22, "b")],
        body,
        Type::Boolean,
    )]);
    // The fixture can be emitted in more than one specialized clone.  Every
    // clone must retain one ordinary construction and two stateful tests.
    let constructions = ir.matches("call i64 @js_regexp_new_site(").count();
    assert!(constructions >= 1, "{ir}");
    assert_eq!(
        ir.matches("call i64 @js_regexp_site_test_new(").count(),
        0,
        "{ir}"
    );
    assert_eq!(
        ir.matches("call i32 @js_regexp_test(").count(),
        constructions * 2,
        "{ir}"
    );
}

fn exact_factory() -> Function {
    function(
        1,
        "factory",
        Vec::new(),
        vec![Stmt::Return(Some(Expr::RegExp {
            pattern: "x".to_string(),
            flags: "g".to_string(),
        }))],
        Type::Named("RegExp".to_string()),
    )
}

#[test]
fn direct_factory_call_records_function_identity_and_uses_the_caller_site() {
    let inner = call(Expr::FuncRef(1), Vec::new());
    let outer = call(property(inner, "test"), vec![Expr::LocalGet(30)]);
    let caller = function(
        2,
        "caller",
        vec![param(30, "s")],
        vec![Stmt::Return(Some(outer))],
        Type::Boolean,
    );
    let ir = compile(vec![exact_factory(), caller]);
    assert!(ir.contains("call i64 @js_regexp_new_factory_site("), "{ir}");
    assert!(
        ir.contains("ptrtoint ptr @perry_fn_"),
        "factory identity missing: {ir}"
    );
    assert!(
        ir.contains("call double @js_regexp_site_factory_call_value("),
        "{ir}"
    );
    assert!(
        ir.contains("call double @js_regexp_site_test_dispatch("),
        "{ir}"
    );
}

#[test]
fn namespace_member_factory_call_uses_the_member_wrapper() {
    // The runtime wrapper resolves `default` first, then activates the site
    // only while invoking the resolved function.  `Undefined` is sufficient
    // for an IR-shape fixture; runtime tests exercise a real namespace object.
    let inner = call(property(Expr::Undefined, "default"), Vec::new());
    let outer = call(property(inner, "test"), vec![Expr::String("x".to_string())]);
    let ir = compile(vec![function(
        1,
        "member",
        Vec::new(),
        vec![Stmt::Return(Some(outer))],
        Type::Any,
    )]);
    assert!(
        ir.contains("call double @js_regexp_site_factory_call_method("),
        "{ir}"
    );
    assert!(
        ir.contains("call double @js_regexp_site_test_dispatch("),
        "{ir}"
    );
}
