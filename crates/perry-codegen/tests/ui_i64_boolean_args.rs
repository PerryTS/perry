//! UI integer ABIs also accept JavaScript booleans at runtime. Boolean values
//! use Perry's NaN-box tags, so they must be converted before an integer cast.

use perry_codegen::{compile_module, CompileOptions};
use perry_hir::{Expr, Module, Stmt};

fn compile_hidden_call(hidden: bool) -> String {
    let mut module = Module::new("ui_i64_boolean_args");
    module.init.push(Stmt::Expr(Expr::NativeMethodCall {
        module: "perry/ui".into(),
        class_name: None,
        object: None,
        method: "widgetSetHidden".into(),
        args: vec![Expr::Number(1.0), Expr::Bool(hidden)],
    }));
    let options = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, options).unwrap()).unwrap()
}

#[test]
fn ui_integer_arguments_convert_boolean_tags_before_casting() {
    for (hidden, expected) in [(false, "false"), (true, "true")] {
        let ir = compile_hidden_call(hidden);
        assert!(
            ir.lines().any(|line| {
                line.contains("zext i1") && line.contains(expected) && line.contains("to i64")
            }),
            "widgetSetHidden({hidden}) must convert the native boolean to i64:\n{ir}"
        );
        assert!(
            !ir.lines().any(|line| {
                line.contains("fptosi double 0x7FFC00000000000") && line.contains("to i64")
            }),
            "widgetSetHidden({hidden}) must not cast a NaN-boxed boolean tag:\n{ir}"
        );
    }
}
