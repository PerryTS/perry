//! #8770 regression: a SAME-MODULE direct call with fewer arguments than the
//! callee's declared parameter count must pad the missing trailing parameters
//! with `TAG_UNDEFINED` — exactly like the cross-module twin
//! (`extern_func.rs`, issue #608 arm) always has.
//!
//! Without the padding, the callee (compiled with `declared_count` double
//! parameters, its default-parameter lowering testing each for `undefined`)
//! reads whatever the caller-saved FP argument registers happen to hold. On
//! the Claude Code bundle — one giant module, so every direct call resolves
//! through the same-module arm — `aP([q])` for `function aP(q, K = !1, _)`
//! handed `K`/`_` the leftovers of `js_array_from_values`' internals:
//! impossible-NaN bit patterns (`0xffffffffffffffff`) that flowed into
//! truthiness tests and method receivers (`_.get(A)`) and crashed in
//! `shape_is_url_search_params` / `js_is_truthy`, or silently corrupted the
//! async iteration ("Detected unsettled top-level await").

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

/// `function callee(a, b, c) { return b; }` called as `callee(7)`.
fn underapplied_call_ir() -> String {
    let callee = function(
        1,
        "callee",
        vec![param(10, "a"), param(11, "b"), param(12, "c")],
        vec![Stmt::Return(Some(Expr::LocalGet(11)))],
    );
    let caller = function(
        2,
        "caller",
        Vec::new(),
        vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::FuncRef(1)),
            args: vec![Expr::Number(7.0)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
    );
    let mut module = Module::new("underapply_pad_test.ts");
    module.functions = vec![callee, caller];
    let opts = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("call fixture must compile"))
        .expect("LLVM IR is UTF-8")
}

#[test]
fn an_underapplied_direct_call_pads_missing_params_with_undefined() {
    let ir = underapplied_call_ir();
    let undefined_lit = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    // The direct call must carry all three declared parameters…
    let call_line = ir
        .lines()
        .find(|line| line.contains("call double @perry_fn_underapply_pad_test_ts__callee("))
        .unwrap_or_else(|| panic!("expected a direct call to the callee:\n{ir}"));
    let args = call_line
        .split("callee(")
        .nth(1)
        .map(|tail| tail.matches("double").count())
        .unwrap_or(0);
    assert!(
        args >= 3,
        "an under-applied direct call must pass every declared parameter \
         (got {args} double args): {call_line}\n{ir}"
    );
    // …and the missing trailing two must be the TAG_UNDEFINED literal.
    assert!(
        call_line.matches(undefined_lit.as_str()).count() >= 2,
        "the two omitted parameters must be padded with the undefined literal \
         {undefined_lit}: {call_line}\n{ir}"
    );
}
