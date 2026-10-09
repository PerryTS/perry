//! The birth rule (`binding_cell`): a closure born where a captured cell
//! binding's root may still hold the entry sentinel checks it and mints the
//! cell; a closure born below a statement of an enclosing list that stored the
//! cell, or capturing a boxed parameter, does neither.
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Param, Stmt};

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.into(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn let_stmt(id: u32, name: &str, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.into(),
        ty: Type::Any,
        mutable: true,
        init: Some(init),
    }
}

/// `() => { <cap> = 42 }`: captures `cap` and mutates it, so `cap` is a cell.
fn writer(func_id: u32, cap: u32) -> Expr {
    Expr::Closure {
        func_id,
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Expr(Expr::LocalSet(cap, Box::new(Expr::Integer(42))))],
        captures: vec![cap],
        mutable_captures: vec![cap],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    }
}

/// The IR of `resume(params) { body }`.
fn resume_ir(params: Vec<Param>, body: Vec<Stmt>) -> String {
    let _shadow = crate::codegen::helpers::NativeRootsPin::shadow();
    let mut module = Module::new("binding_cell.ts");
    module.functions.push(Function {
        id: 1,
        name: "resume".into(),
        type_params: Vec::new(),
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let ir = String::from_utf8(
        crate::compile_module(&module, super::prealloc_module_global_tests::ir_opts()).unwrap(),
    )
    .unwrap();
    let start = ir
        .lines()
        .position(|l| l.starts_with("define") && l.contains("__resume("))
        .unwrap_or_else(|| panic!("no resume body:\n{ir}"));
    ir.lines()
        .skip(start)
        .take_while(|l| *l != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

fn mint_blocks(ir: &str) -> usize {
    ir.lines()
        .filter(|l| l.starts_with("capture_cell.mint"))
        .count()
}

#[test]
fn a_birth_below_its_declaration_trusts_the_root() {
    let ir = resume_ir(
        vec![param(100, "state")],
        vec![
            let_stmt(101, "cell", Expr::Integer(1)),
            let_stmt(102, "w", writer(2, 101)),
            Stmt::Return(Some(Expr::LocalGet(101))),
        ],
    );
    assert!(
        ir.contains("@js_closure_alloc_init_boxed("),
        "premise: the closure is born with a cell capture:\n{ir}"
    );
    assert_eq!(mint_blocks(&ir), 0, "dominated root needs no check:\n{ir}");
}

#[test]
fn a_birth_capturing_a_boxed_parameter_trusts_the_root() {
    let ir = resume_ir(
        vec![param(100, "state"), param(103, "p")],
        vec![
            let_stmt(102, "w", writer(2, 103)),
            Stmt::Return(Some(Expr::LocalGet(103))),
        ],
    );
    assert!(
        ir.contains("@js_closure_alloc_init_boxed("),
        "premise: the closure is born with a cell capture:\n{ir}"
    );
    assert_eq!(
        mint_blocks(&ir),
        0,
        "entry-boxed parameter needs no check:\n{ir}"
    );
}

#[test]
fn a_birth_its_declaration_may_not_have_reached_mints_the_cell() {
    // `if (state) { let cell = 1 }` stores the cell only on one path; the
    // closure born after the `if` must not install the entry sentinel.
    let ir = resume_ir(
        vec![param(100, "state")],
        vec![
            Stmt::If {
                condition: Expr::LocalGet(100),
                then_branch: vec![let_stmt(101, "cell", Expr::Integer(1))],
                else_branch: None,
            },
            let_stmt(102, "w", writer(2, 101)),
            Stmt::Return(Some(Expr::LocalGet(101))),
        ],
    );
    assert_eq!(mint_blocks(&ir), 1, "one checked capture:\n{ir}");
    let mint = ir.split("\ncapture_cell.mint").nth(1).expect("mint block");
    let mint = mint.split("\n\n").next().unwrap_or(mint);
    assert!(
        mint.contains("@js_box_alloc_bits(") && mint.contains("store i64"),
        "the mint arm allocates the cell and publishes it into the root:\n{mint}"
    );
}
