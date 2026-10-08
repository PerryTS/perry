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
    assert!(
        ir.contains("private global [2 x i64] zeroinitializer"),
        "{ir}"
    );
    assert_generic(&ir);
}

/// The register a `getelementptr i8, ptr <base>, i64 <offset>` line defines.
fn gep_reg(ir: &str, offset: &str) -> Vec<String> {
    ir.lines()
        .filter(|l| {
            l.contains("getelementptr i8, ptr")
                && l.trim_end().ends_with(&format!(", i64 {offset}"))
        })
        .filter_map(|l| l.trim().split(" = ").next().map(str::to_string))
        .collect()
}

/// A literal evaluation is an inline birth: the agent gate and the site's
/// header word, a bump of the inline arena, every slot initialized, and only
/// then the birth seed (the runtime call is the slow arm).
#[test]
fn literal_birth_initializes_every_slot_before_the_birth_seed() {
    let ir = compile(vec![function(
        1,
        "fresh",
        Vec::new(),
        vec![Stmt::Return(Some(Expr::RegExp {
            pattern: "x".into(),
            flags: "".into(),
        }))],
        Type::Any,
    )]);
    let body = &ir[ir.find("define").expect("a function")..];
    assert!(
        body.contains("load atomic i8, ptr @PERRY_METHOD_SITE_WORKERS_PRESENT seq_cst"),
        "{ir}"
    );
    assert!(body.contains("call i64 @js_regexp_literal("), "{ir}");
    let seed = body
        .find("call void @js_gc_note_black_birth(")
        .expect("the birth seed");
    // Slot 0 (matcher data) at +24 and slot 1 (lastIndex) at +32 from the
    // raw cell: both stores precede the seed.
    // Only STORE lines count: the arena state's own fields (+24 birth flags,
    // +32 seed queue) are loaded through geps of the same offsets.
    let store_at = |reg: &str| {
        let mut at = 0;
        body.lines().find_map(|line| {
            let here = at;
            at += line.len() + 1;
            let line = line.trim();
            (line.starts_with("store i64 ") && line.ends_with(&format!(", ptr {reg}")))
                .then_some(here)
        })
    };
    for offset in ["24", "32"] {
        let stored = gep_reg(body, offset)
            .iter()
            .filter_map(|reg| store_at(reg))
            .min()
            .unwrap_or_else(|| panic!("no store through the +{offset} slot: {ir}"));
        assert!(
            stored < seed,
            "slot +{offset} stored after the birth seed: {ir}"
        );
    }
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
