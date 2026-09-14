use perry_codegen::{compile_module, CompileOptions};
use perry_hir::{Expr, Module, Stmt};

#[test]
fn max_width_lowers_to_the_native_widget_abi() {
    let mut module = Module::new("widget_max_width");
    module.init.push(Stmt::Expr(Expr::NativeMethodCall {
        module: "perry/ui".into(),
        class_name: None,
        object: None,
        method: "widgetSetMaxWidth".into(),
        args: vec![Expr::Number(1.0), Expr::Number(640.0)],
    }));
    let options = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let ir = String::from_utf8(compile_module(&module, options).unwrap()).unwrap();
    assert!(
        ir.lines().any(
            |line| line.contains("call void @perry_ui_widget_set_max_width(i64 ")
                && line.contains(", double ")
        ),
        "max-width must pass a native widget handle and numeric cap: {ir}"
    );
}
