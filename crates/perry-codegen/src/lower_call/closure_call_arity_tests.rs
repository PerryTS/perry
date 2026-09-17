//! #10420: calls through a function VALUE have no argument-count ceiling.
//!
//! A closure-typed local called with more than 16 arguments used to fail to
//! compile (`closure call with 18 args (max 16)`), and the `__perry_wrap_*`
//! value wrappers of top-level functions were capped at 16 params, so every
//! dynamic call of an 18-param function (`apply`/`call`/spread/object-literal
//! method) passed `0` for params 17 and 18. Up to 16 arguments must keep the
//! per-arity `js_closure_call{N}` fast path; wider calls marshal a stack buffer
//! into `js_closure_call_array`.

use crate::{compile_module, CompileOptions};
use perry_hir::types::{FunctionType, Type};
use perry_hir::{Expr, Function, Module, Param, Stmt};

fn param(id: u32) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn params(base: u32, count: usize) -> Vec<Param> {
    (0..count as u32).map(|i| param(base + i)).collect()
}

fn function_type(count: usize) -> Type {
    Type::Function(FunctionType {
        params: (0..count)
            .map(|i| (format!("p{i}"), Type::Any, false))
            .collect(),
        return_type: Box::new(Type::Any),
        is_async: false,
        is_generator: false,
    })
}

fn ir(module: &Module) -> String {
    let opts = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    String::from_utf8(compile_module(module, opts).expect("fixture must compile"))
        .expect("LLVM IR is UTF-8")
}

/// `const f: (p0, …) => any = (p0, …) => p<last>; f(1, 2, …, argc)`.
fn closure_value_call_ir(file: &str, argc: usize) -> String {
    let mut module = Module::new(file);
    module.init.push(Stmt::Let {
        id: 1,
        name: "f".to_string(),
        ty: function_type(argc),
        mutable: false,
        init: Some(Expr::Closure {
            func_id: 7,
            params: params(100, argc),
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::LocalGet(100 + argc as u32 - 1)))],
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: true,
        }),
    });
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::LocalGet(1)),
        args: (1..=argc).map(|i| Expr::Number(i as f64)).collect(),
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    ir(&module)
}

#[test]
fn a_wide_closure_value_call_compiles_through_the_array_path() {
    let ir = closure_value_call_ir("closure_call_arity_wide.ts", 18);
    assert!(
        ir.contains("call double @js_closure_call_array(i64 ") && ir.contains(", i64 18)"),
        "an 18-argument closure-value call must dispatch through \
         js_closure_call_array with argc 18:\n{ir}"
    );
    assert!(
        !ir.contains("@js_closure_call18("),
        "no fixed-arity entry point exists past 16:\n{ir}"
    );
}

#[test]
fn a_narrow_closure_value_call_keeps_the_fixed_arity_fast_path() {
    let ir = closure_value_call_ir("closure_call_arity_narrow.ts", 3);
    assert!(
        ir.contains("call double @js_closure_call3(i64 "),
        "a 3-argument closure-value call must keep js_closure_call3:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_closure_call_array("),
        "a 3-argument call must not take the array path:\n{ir}"
    );
}

#[test]
fn function_value_wrappers_take_every_declared_param() {
    let mut module = Module::new("closure_call_arity_wrapper.ts");
    module.functions = vec![Function {
        id: 1,
        name: "wide".to_string(),
        type_params: Vec::new(),
        params: params(10, 18),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::LocalGet(27)))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    }];
    let ir = ir(&module);
    let define = ir
        .lines()
        .find(|line| {
            line.starts_with("define") && line.contains("@__perry_wrap_") && line.contains("wide(")
        })
        .unwrap_or_else(|| panic!("expected the value wrapper of `wide`:\n{ir}"));
    assert_eq!(
        define.matches("double %a").count(),
        18,
        "the wrapper must forward all 18 params, not a 16-param prefix: {define}"
    );
}
