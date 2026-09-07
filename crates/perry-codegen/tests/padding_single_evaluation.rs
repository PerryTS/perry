//! Uniform padding is one JavaScript expression even though the native ABI
//! receives four edge values. These IR tests keep both public call forms from
//! cloning and re-evaluating a side-effecting expression.

use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Stmt};

fn value_call() -> Expr {
    Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

fn module_with_padding_call(call: Expr) -> Module {
    let mut module = Module::new("padding_single_evaluation");
    module.functions.push(Function {
        id: 1,
        name: "next_padding".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::Number(7.0)))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    module.functions.push(Function {
        id: 2,
        name: "apply_padding".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Number,
        body: vec![Stmt::Expr(call), Stmt::Return(Some(Expr::Number(0.0)))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    module
}

fn compile_ir(call: Expr) -> String {
    let options = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    String::from_utf8(compile_module(&module_with_padding_call(call), options).unwrap()).unwrap()
}

fn assert_one_value_evaluation(ir: &str) {
    let body = ir
        .split("define double @perry_fn_padding_single_evaluation__apply_padding()")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("apply_padding body is present");
    // A direct call has one typed arm and one generic fallback arm in the IR;
    // only one arm executes. Cloning the source expression four times emits
    // four such pairs, which is the regression this count detects.
    let calls = body
        .lines()
        .filter(|line| {
            line.contains("call double @perry_fn_padding_single_evaluation__next_padding$")
        })
        .count();
    assert_eq!(
        calls, 2,
        "uniform padding must evaluate its value once:\n{ir}"
    );
}

#[test]
fn free_function_uniform_padding_evaluates_value_once() {
    let ir = compile_ir(Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: "setPadding".to_string(),
        args: vec![Expr::Number(1.0), value_call()],
    });
    assert_one_value_evaluation(&ir);
}

#[test]
fn instance_uniform_padding_evaluates_value_once() {
    let ir = compile_ir(Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: Some(Box::new(Expr::Number(1.0))),
        method: "setPadding".to_string(),
        args: vec![value_call()],
    });
    assert_one_value_evaluation(&ir);
}
