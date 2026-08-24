//! Exact-`undefined` optional method versioning.

use crate::{compile_module, CompileOptions};
use perry_hir::types::{FunctionType, Type};
use perry_hir::{Class, CompareOp, Expr, Function, LogicalOp, Module, Param, Stmt};

const FILTER: u32 = 20;

fn callback_type() -> Type {
    Type::Function(FunctionType {
        params: vec![("value".to_string(), Type::Any, false)],
        return_type: Box::new(Type::Boolean),
        is_async: false,
        is_generator: false,
    })
}

fn optional_filter() -> Param {
    Param {
        id: FILTER,
        name: "filter".to_string(),
        ty: callback_type(),
        default: Some(Expr::Undefined),
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn synthetic_optional_prologue() -> Stmt {
    Stmt::If {
        condition: Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(Expr::LocalGet(FILTER)),
            right: Box::new(Expr::Undefined),
        },
        then_branch: vec![Stmt::Expr(Expr::LocalSet(
            FILTER,
            Box::new(Expr::Undefined),
        ))],
        else_branch: None,
    }
}

fn loop_filter_guard() -> Stmt {
    Stmt::For {
        init: None,
        condition: Some(Expr::Bool(false)),
        update: None,
        body: vec![Stmt::If {
            condition: Expr::Logical {
                op: LogicalOp::And,
                left: Box::new(Expr::LocalGet(FILTER)),
                right: Box::new(Expr::Call {
                    callee: Box::new(Expr::LocalGet(FILTER)),
                    args: vec![Expr::Integer(1)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }),
            },
            then_branch: vec![Stmt::Expr(Expr::Integer(1))],
            else_branch: None,
        }],
    }
}

fn method(extra: Vec<Stmt>) -> Function {
    let mut body = vec![synthetic_optional_prologue(), loop_filter_guard()];
    body.extend(extra);
    Function {
        id: 90,
        name: "scan".to_string(),
        type_params: Vec::new(),
        params: vec![optional_filter()],
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
        name: "Scanner".to_string(),
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

fn emit(method: Function) -> String {
    let mut module = Module::new("guarded_undefined_method.ts");
    module.classes = vec![class(method)];
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("fixture compiles"))
        .expect("LLVM IR is UTF-8")
}

fn function_body<'a>(ir: &'a str, marker: &str) -> &'a str {
    let start = ir
        .match_indices("define ")
        .find(|(index, _)| {
            let end = ir[*index..]
                .find('\n')
                .map(|offset| index + offset)
                .unwrap_or(ir.len());
            ir[*index..end].contains(marker)
        })
        .map(|(index, _)| index)
        .unwrap_or_else(|| panic!("missing function containing {marker}:\n{ir}"));
    let end = ir[start..]
        .find("\n}")
        .map(|offset| start + offset)
        .expect("function terminator");
    &ir[start..end]
}

#[test]
fn wrapper_guards_actual_bits_and_clone_erases_the_loop_filter_arm() {
    let ir = emit(method(Vec::new()));
    let base = "perry_method_guarded_undefined_method_ts__Scanner__scan";
    let wrapper = function_body(&ir, &format!("@{base}("));
    let generic = function_body(&ir, &format!("@{base}$generic("));
    let undefined = function_body(&ir, &format!("@{base}$undef0("));

    assert!(
        wrapper.starts_with("define double "),
        "the guarded wrapper is a published capability; only its bodies are private:\n{wrapper}"
    );
    assert!(wrapper.contains(&crate::nanbox::TAG_UNDEFINED_I64.to_string()));
    assert!(wrapper.contains(&format!("@{base}$undef0(")));
    assert!(wrapper.contains(&format!("@{base}$generic(")));
    assert!(generic.contains("@js_is_truthy("), "{generic}");
    assert!(generic.contains("@js_closure_call1("), "{generic}");
    assert!(
        !undefined.contains("@js_is_truthy("),
        "the guarded clone must not test the known-falsy filter in its loop:\n{undefined}"
    );
    assert!(
        !undefined.contains("@js_closure_call1("),
        "the conditional filter call must be unreachable in the clone:\n{undefined}"
    );
}

#[test]
fn a_real_parameter_write_keeps_one_unspecialized_public_body() {
    let mut candidate = method(Vec::new());
    candidate.body.push(Stmt::Expr(Expr::LocalSet(
        FILTER,
        Box::new(Expr::Bool(true)),
    )));
    let ir = emit(candidate);
    let base = "perry_method_guarded_undefined_method_ts__Scanner__scan";
    let public = function_body(&ir, &format!("@{base}("));
    assert!(public.contains("@js_is_truthy("));
    assert!(!ir.contains(&format!("@{base}$undef0(")));
    assert!(!ir.contains(&format!("@{base}$generic(")));
}

#[test]
fn an_oversized_method_does_not_consume_the_full_body_clone_budget() {
    let padding = (0..1_025)
        .map(|value| Stmt::Expr(Expr::Integer(value)))
        .collect();
    assert!(super::param_guard::guarded_undefined_method_candidate(&method(padding)).is_none());
}
