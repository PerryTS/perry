//! Regression coverage for native-module operands surviving later arguments.
//!
//! `https.createServer(options, wrapper(handler))` exposed this as an options
//! object that arrived at the runtime as an evacuated `{}`: native-table calls
//! lowered each argument into an unrooted SSA register, and a later allocating
//! argument could move an earlier heap value before the FFI call consumed it.

use crate::testing::temp_slots::{assert_rooted_across, first_call_result};
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Stmt};

fn compile_native_call(args: Vec<Expr>) -> String {
    let mut module = Module::new("native_module_rooting_test.ts");
    module.functions.push(Function {
        id: 0,
        name: "build".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Expr(Expr::NativeMethodCall {
            module: "https".to_string(),
            class_name: None,
            object: None,
            method: "createServer".to_string(),
            args,
        })],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            },
        )
        .expect("native call fixture compiles"),
    )
    .expect("LLVM IR is UTF-8")
}

fn build_function_ir(module_ir: &str) -> &str {
    let start = module_ir
        .find("define double @perry_fn_native_module_rooting_test_ts__build(")
        .unwrap_or_else(|| panic!("build function was not emitted:\n{module_ir}"));
    let tail = &module_ir[start..];
    let end = tail
        .find("\n}\n")
        .unwrap_or_else(|| panic!("build function was not terminated:\n{tail}"));
    &tail[..end + 3]
}

#[test]
fn native_module_first_argument_is_rooted_across_allocating_second_argument() {
    let module_ir = compile_native_call(vec![
        Expr::Object(vec![("key".to_string(), Expr::String("pem".to_string()))]),
        Expr::Object(vec![("handler".to_string(), Expr::Number(1.0))]),
    ]);
    let ir = build_function_ir(&module_ir);
    let options_raw = first_call_result(&ir, "js_object_alloc_with_shape")
        .unwrap_or_else(|| panic!("the options argument must allocate:\n{ir}"));
    let options = ir
        .lines()
        .find_map(|line| {
            let (register, definition) = line.trim().split_once(" = ")?;
            definition
                .starts_with(&format!("or i64 {options_raw}, "))
                .then(|| register.to_string())
        })
        .unwrap_or_else(|| panic!("the options allocation must be NaN-boxed:\n{ir}"));

    assert_rooted_across(
        &ir,
        &options,
        "js_node_https_create_server",
        "native-module options argument",
    );
}
