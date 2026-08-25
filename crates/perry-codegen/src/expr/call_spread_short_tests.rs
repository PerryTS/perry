//! IR ratchets for guarded short packed-spread calls (#8772).

use perry_hir::types::Type;
use perry_hir::{CallArg, Class, Expr, Function, Module, ModuleInitKind, Param, Stmt};

fn param(id: u32, name: &str, ty: Type, default: Option<Expr>) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty,
        default,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn reset(id: u32, default: f64) -> Function {
    Function {
        id,
        name: "reset".to_string(),
        type_params: Vec::new(),
        params: vec![
            param(id + 1, "entity", Type::Any, None),
            param(id + 2, "delta", Type::Number, Some(Expr::Number(default))),
        ],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::LocalGet(id + 2)))],
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

fn class(id: u32, name: &str, default: f64) -> Class {
    Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: vec![reset(id + 10, default)],
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

fn fixture() -> Module {
    let mut module = Module::new("issue_8772_short_spread.ts");
    module.classes = vec![class(100, "Position", 1.0), class(200, "Velocity", 2.0)];
    module.functions = vec![Function {
        id: 1,
        name: "invoke".to_string(),
        type_params: Vec::new(),
        params: vec![
            param(2, "instance", Type::Any, None),
            param(3, "entity", Type::Any, None),
            param(4, "args", Type::Array(Box::new(Type::Any)), None),
        ],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::CallSpread {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(2)),
                property: "reset".to_string(),
                byte_offset: 0,
            }),
            args: vec![
                CallArg::Expr(Expr::LocalGet(3)),
                CallArg::Spread(Expr::LocalGet(4)),
            ],
            type_args: Vec::new(),
        }))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    module.init_kind = ModuleInitKind::Eager;
    module
}

fn emit() -> String {
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    String::from_utf8(crate::compile_module(&fixture(), opts).expect("fixture compiles"))
        .expect("LLVM IR is UTF-8")
}

fn function_body<'a>(ir: &'a str, fragment: &str) -> &'a str {
    let name = ir
        .find(fragment)
        .unwrap_or_else(|| panic!("missing function {fragment:?}\n{ir}"));
    let start = ir[..name]
        .rfind("\ndefine ")
        .unwrap_or_else(|| panic!("missing definition for {fragment:?}"));
    let tail = &ir[start + 1..];
    let end = tail.find("\n}\n").expect("terminated function definition");
    &tail[..end + 2]
}

fn named_block<'a>(function: &'a str, label: &str) -> &'a str {
    let needle = format!("\n{label}");
    let start = function
        .find(&needle)
        .map(|offset| offset + 1)
        .unwrap_or_else(|| panic!("missing block {label:?}\n{function}"));
    let tail = &function[start..];
    let end = tail[1..]
        .find("\nshort_spread.")
        .map(|offset| offset + 1)
        .unwrap_or(tail.len());
    &tail[..end]
}

#[test]
fn every_short_packed_arity_calls_reset_directly_without_apply() {
    let ir = emit();
    let invoke = function_body(&ir, "__invoke(");
    assert!(invoke.contains("call i32 @js_short_packed_spread_values("));
    assert!(invoke.contains("call i32 @js_method_direct_shape_class("));

    for candidate in 0..2 {
        for arity in 0..=4 {
            let block = named_block(
                invoke,
                &format!("short_spread.target{candidate}.arity{arity}"),
            );
            assert!(
                block.contains("call double @perry_method_") && block.contains("__reset("),
                "packed arity {arity} must call a selected reset body directly\n{block}"
            );
            assert!(
                !block.contains("js_native_call_method_apply")
                    && !block.contains("js_spread_tail_fallback_args"),
                "packed arity {arity} must contain no apply machinery\n{block}"
            );
        }
    }
}

#[test]
fn guard_misses_retain_one_full_iterator_apply_fallback() {
    let ir = emit();
    let invoke = function_body(&ir, "__invoke(");
    let fallback = named_block(invoke, "short_spread.fallback");
    assert_eq!(
        fallback
            .matches("call i64 @js_spread_tail_fallback_args(")
            .count(),
        1,
        "fallback must materialize fixed+spread exactly once\n{fallback}"
    );
    assert_eq!(
        fallback
            .matches("call double @js_native_call_method_apply_by_id(")
            .count(),
        1,
        "fallback must retain dynamic method apply\n{fallback}"
    );
}
