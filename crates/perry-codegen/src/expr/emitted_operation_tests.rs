//! Machine instruction ratchets for a single exported operation, including
//! its ABI and stack guard. Semantic coverage lives in the gap fixture.

use crate::{compile_module, CompileOptions};
use perry_hir::{types::Type, CompareOp, Expr, Function, Module, Param, Stmt};

fn assembly(expr: Expr) -> String {
    let mut module = Module::new("emit_cost");
    module.functions.push(Function {
        id: 1,
        name: "probe".into(),
        type_params: vec![],
        params: (1..=2)
            .map(|id| Param {
                id,
                name: format!("p{id}"),
                ty: Type::Any,
                default: None,
                decorators: vec![],
                is_rest: false,
                arguments_object: None,
            })
            .collect(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(expr))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    let _roots = crate::codegen::helpers::NativeRootsPin::native();
    let ir = compile_module(
        &module,
        CompileOptions {
            emit_ir_only: true,
            is_entry_module: false,
            target: Some("x86_64-unknown-linux-gnu".into()),
            ..CompileOptions::default()
        },
    )
    .unwrap();
    let context = inkwell::context::Context::create();
    let module =
        crate::inprocess::parse_ir_text(&context, std::str::from_utf8(&ir).unwrap(), "emit_cost")
            .unwrap();
    let output = crate::inprocess::optimize_and_emit_module(
        &module,
        "x86_64-unknown-linux-gnu",
        &["-O3".into(), "-S".into()],
        true,
    )
    .unwrap();
    let output = String::from_utf8(crate::inprocess::single_piece(output)).unwrap();
    let start = output
        .find("perry_fn_emit_cost__probe:")
        .expect("exported probe is emitted");
    let end = output[start..].find(".Lfunc_end").expect("function end");
    output[start..start + end].to_string()
}

fn instruction_count(asm: &str) -> usize {
    asm.lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|line| !line.is_empty() && !line.starts_with('.') && !line.ends_with(':'))
        .count()
}

#[test]
fn literal_integer_compare_emits_at_most_24_instructions() {
    let asm = assembly(Expr::Compare {
        op: CompareOp::Eq,
        left: Box::new(Expr::LocalGet(1)),
        right: Box::new(Expr::Number(80.0)),
    });
    assert!(
        instruction_count(&asm) <= 24,
        "one compare, boxing and ABI: {asm}"
    );
    assert!(!asm.contains("js_eq"), "{asm}");
    assert!(
        !asm.contains("9222809086901354576"),
        "no compact 80 re-compare: {asm}"
    );
}

#[test]
fn scalar_store_emits_no_barrier_and_at_most_170_instructions() {
    let value = Expr::Binary {
        op: perry_hir::BinaryOp::BitOr,
        left: Box::new(Expr::LocalGet(2)),
        right: Box::new(Expr::Number(0.0)),
    };
    let store = |value| Expr::PropertySet {
        object: Box::new(Expr::LocalGet(1)),
        property: "pos".into(),
        value: Box::new(value),
    };
    let asm = assembly(store(value));
    assert!(
        instruction_count(&asm) <= 170,
        "scalar store and both IC routes: {asm}"
    );
    assert!(!asm.contains("write_barrier"), "{asm}");
    let control = assembly(store(Expr::LocalGet(2)));
    assert!(
        control.contains("write_barrier"),
        "unknown-value negative control: {control}"
    );
    assert!(
        instruction_count(&control) > instruction_count(&asm),
        "{control}"
    );
}
