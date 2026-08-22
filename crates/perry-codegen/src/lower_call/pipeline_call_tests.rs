//! Regression coverage for the two call-site proofs used by `pipeline`.

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Param, Stmt};

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn function(id: u32, name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    }
}

fn alias_call_ir(mutable: bool) -> String {
    let identity = function(
        1,
        "identity",
        vec![param(10, "value")],
        vec![Stmt::Return(Some(Expr::LocalGet(10)))],
    );
    let caller = function(
        2,
        "caller",
        Vec::new(),
        vec![
            Stmt::Let {
                id: 20,
                name: "idf".to_string(),
                ty: Type::Any,
                mutable,
                init: Some(Expr::FuncRef(1)),
            },
            Stmt::Return(Some(Expr::Call {
                callee: Box::new(Expr::LocalGet(20)),
                args: vec![Expr::Number(7.0)],
                type_args: Vec::new(),
                byte_offset: 0,
            })),
        ],
    );
    let mut module = Module::new("pipeline_call_test.ts");
    module.functions = vec![identity, caller];
    let opts = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("call fixture must compile"))
        .expect("LLVM IR is UTF-8")
}

#[test]
fn immutable_function_alias_calls_the_known_symbol_directly() {
    let ir = alias_call_ir(false);
    assert!(
        ir.contains("call double @perry_fn_pipeline_call_test_ts__identity(double"),
        "an immutable FuncRef alias must use direct function lowering:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_closure_call1_receiverless"),
        "the immutable alias must not retain dynamic closure dispatch:\n{ir}"
    );
}

#[test]
fn mutable_function_alias_keeps_guarded_receiverless_dispatch() {
    let ir = alias_call_ir(true);
    assert!(
        ir.contains("call double @js_closure_call1_receiverless"),
        "a mutable alias has no stable target and must retain dynamic dispatch:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_implicit_this_set"),
        "the arrow-aware runtime dispatcher owns receiverless this binding:\n{ir}"
    );
}
