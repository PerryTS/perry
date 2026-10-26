use super::*;
use perry_hir::{Expr, Module, Stmt};

fn fixture() -> Module {
    let mut module = Module::new("ownership");
    module.init.push(Stmt::Expr(Expr::Object(vec![(
        "value".into(),
        Expr::Number(42.0),
    )])));
    module
}

#[test]
fn lowering_releases_analysis_options_before_emission() {
    let module = fixture();
    let mut opts = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    ensure_layout_plan(&module, &mut opts).unwrap();
    let layouts = Arc::clone(&opts.program_layouts);
    let lowered = lower_module(&module, opts, None).unwrap().unwrap();
    assert_eq!(
        Arc::strong_count(&layouts),
        1,
        "LLVM emission must not retain the lowering options graph"
    );
    let expected = emit_lowered_module(lowered).unwrap();
    let actual = compile_module_owned(
        module,
        CompileOptions {
            emit_ir_only: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(expected, actual, "owned and borrowed HIR emit identical IR");
}
