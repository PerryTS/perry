//! Entry-hoisted arrow callback dispatch and fail-closed fallback ratchets.

use crate::{compile_module, CompileOptions};
use perry_hir::types::{FunctionType, Type};
use perry_hir::{Class, Expr, Function, Module, ModuleInitKind, Param, Stmt};

const CALLBACK_PARAM: u32 = 20;
const CALLBACK_ALIAS: u32 = 21;

fn param(id: u32, name: &str, ty: Type) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn callback_type() -> Type {
    Type::Function(FunctionType {
        params: vec![
            ("a".into(), Type::Any, false),
            ("b".into(), Type::Any, false),
            ("c".into(), Type::Any, false),
        ],
        return_type: Box::new(Type::Void),
        is_async: false,
        is_generator: false,
    })
}

fn method(body: Vec<Stmt>) -> Function {
    Function {
        id: 90,
        name: "visit".to_string(),
        type_params: Vec::new(),
        params: vec![param(CALLBACK_PARAM, "callback", callback_type())],
        return_type: Type::Void,
        body,
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

fn class(method: Function) -> Class {
    Class {
        id: 100,
        name: "Iterator".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: vec![method],
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

fn call(local: u32, arity: usize) -> Stmt {
    Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::LocalGet(local)),
        args: (0..arity)
            .map(|value| Expr::Integer(value as i64))
            .collect(),
        type_args: Vec::new(),
        byte_offset: 0,
    })
}

fn alias_then_call() -> Vec<Stmt> {
    vec![
        Stmt::Let {
            id: CALLBACK_ALIAS,
            name: "cb".to_string(),
            ty: callback_type(),
            mutable: false,
            init: Some(Expr::LocalGet(CALLBACK_PARAM)),
        },
        call(CALLBACK_ALIAS, 3),
    ]
}

fn emit(method: Function) -> String {
    let mut module = Module::new("hoisted_callback_method.ts");
    module.classes = vec![class(method)];
    module.init_kind = ModuleInitKind::Eager;
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("fixture compiles"))
        .expect("LLVM IR is UTF-8")
}

fn function_body(ir: &str, definition_contains: &str) -> String {
    let start = ir
        .lines()
        .position(|line| line.starts_with("define") && line.contains(definition_contains))
        .unwrap_or_else(|| panic!("no definition containing {definition_contains:?}:\n{ir}"));
    ir.lines()
        .skip(start)
        .take_while(|line| *line != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn immutable_alias_resolves_once_without_an_identity_guard_and_keeps_the_fallback() {
    let ir = emit(method(alias_then_call()));
    let body = function_body(
        &ir,
        "@perry_method_hoisted_callback_method_ts__Iterator__visit(",
    );
    assert_eq!(
        body.matches("@js_closure_resolve_arrow_direct_call(")
            .count(),
        1,
        "the method must resolve one target before all callback calls:\n{body}"
    );
    assert!(body.contains("i32 3"));
    assert!(body.contains("icmp ne ptr"));
    assert!(
        !body.contains("icmp eq i64"),
        "an exact const alias retains the resolved parameter identity:\n{body}"
    );
    assert!(
        body.lines()
            .any(|line| line.contains(" = call double %") && line.contains("i64 ")),
        "the admitted arm must use an opaque-pointer indirect call:\n{body}"
    );
    assert!(
        body.contains("@js_closure_call3("),
        "a null/mismatched target must retain full receiverless dispatch:\n{body}"
    );
    assert!(!body.contains("$callback_"));
}

#[test]
fn direct_parameter_call_needs_no_alias_identity_guard() {
    let ir = emit(method(vec![call(CALLBACK_PARAM, 3)]));
    let body = function_body(
        &ir,
        "@perry_method_hoisted_callback_method_ts__Iterator__visit(",
    );
    assert_eq!(
        body.matches("@js_closure_resolve_arrow_direct_call(")
            .count(),
        1
    );
    assert!(body.contains("icmp ne ptr"));
    assert!(
        !body.contains("icmp eq i64"),
        "the immutable source parameter is already the resolved identity:\n{body}"
    );
    assert!(body
        .lines()
        .any(|line| line.contains(" = call double %") && line.contains("i64 ")));
    assert!(body.contains("@js_closure_call3("));
}

#[test]
fn reassigned_callback_parameter_is_not_hoisted() {
    let mut body = alias_then_call();
    body.insert(
        0,
        Stmt::Expr(Expr::LocalSet(CALLBACK_PARAM, Box::new(Expr::Undefined))),
    );
    let ir = emit(method(body));
    let body = function_body(
        &ir,
        "@perry_method_hoisted_callback_method_ts__Iterator__visit(",
    );
    assert!(!body.contains("@js_closure_resolve_arrow_direct_call("));
    assert!(body.contains("@js_closure_call3("));
}

#[test]
fn more_than_four_call_arities_declines_the_parameter() {
    let body = (0..=4).map(|arity| call(CALLBACK_PARAM, arity)).collect();
    let ir = emit(method(body));
    let body = function_body(
        &ir,
        "@perry_method_hoisted_callback_method_ts__Iterator__visit(",
    );
    assert!(!body.contains("@js_closure_resolve_arrow_direct_call("));
    for arity in 0..=4 {
        assert!(body.contains(&format!("@js_closure_call{arity}(")));
    }
}

#[test]
fn nested_closure_calls_are_not_assigned_the_outer_methods_ssa_target() {
    let nested_param = Param {
        default: Some(Expr::Call {
            callee: Box::new(Expr::LocalGet(CALLBACK_PARAM)),
            args: vec![Expr::Integer(1), Expr::Integer(2), Expr::Integer(3)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
        ..param(701, "value", Type::Any)
    };
    let nested = Expr::Closure {
        func_id: 700,
        params: vec![nested_param],
        return_type: Type::Void,
        body: vec![call(CALLBACK_PARAM, 3)],
        captures: vec![CALLBACK_PARAM],
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    };
    let ir = emit(method(vec![Stmt::Expr(nested)]));
    let body = function_body(
        &ir,
        "@perry_method_hoisted_callback_method_ts__Iterator__visit(",
    );
    assert!(!body.contains("@js_closure_resolve_arrow_direct_call("));
}
