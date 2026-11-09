//! wpfacts-proven Number bindings (`codegen/facts_number_locals.rs`): a
//! parameter or `let` the facts prove holds only Numbers is a Number by
//! construction, so its arithmetic and comparisons take the raw-double
//! lowering and it owns no root slot. A binding the facts do not prove keeps
//! the guarded lowering and its slot.

use crate::testing::root_slots::{function_slice, value_slot_binds};
use crate::testing::NativeRootsPin;
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt};

fn param(id: u32) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty: Type::Any,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

fn bin(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn let_(id: u32, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("l{id}"),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

/// `probe(a, b) { const s = a + b; const t = s * 2; return t < 100 ? t : s }`
fn module() -> Module {
    let mut m = Module::new("facts_num.ts");
    m.functions.push(Function {
        id: 1,
        name: "probe".into(),
        type_params: vec![],
        params: vec![param(1), param(2)],
        return_type: Type::Any,
        body: vec![
            let_(3, bin(BinaryOp::Add, Expr::LocalGet(1), Expr::LocalGet(2))),
            let_(4, bin(BinaryOp::Mul, Expr::LocalGet(3), Expr::Integer(2))),
            Stmt::Return(Some(Expr::Conditional {
                condition: Box::new(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(4)),
                    right: Box::new(Expr::Integer(100)),
                }),
                then_expr: Box::new(Expr::LocalGet(4)),
                else_expr: Box::new(Expr::LocalGet(3)),
            })),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    m
}

fn ir_with(numbers: &[u32]) -> String {
    let mut facts = crate::codegen::ElisionFacts::default();
    facts.number_inputs.extend(numbers.iter().copied());
    String::from_utf8(
        compile_module(
            &module(),
            CompileOptions {
                emit_ir_only: true,
                is_entry_module: false,
                program_facts: crate::codegen::ProgramFactsView {
                    elision: std::sync::Arc::new(facts),
                    ..Default::default()
                },
                ..CompileOptions::default()
            },
        )
        .expect("module compiles"),
    )
    .unwrap()
}

fn dynamic_calls(body: &str) -> usize {
    body.lines()
        .filter(|l| {
            l.contains("@js_dynamic_") || l.contains("@js_rel_") || l.contains("@js_to_numeric")
        })
        .count()
}

#[test]
fn proven_bindings_take_raw_double_ops_and_own_no_root_slot() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_with(&[1, 2, 3, 4]);
    let body = function_slice(&ir, "perry_fn_facts_num_ts__probe");
    assert_eq!(
        dynamic_calls(body),
        0,
        "proven Numbers need no dynamic arm:\n{body}"
    );
    assert_eq!(
        value_slot_binds(body),
        0,
        "a proven Number binding never holds a pointer:\n{body}"
    );
    // P3 B1, C4: a proven Number is a plain double (the NUM fact is audited,
    // `perry-wpfacts/NUM_SEEDS.md`), so the parameter takes its argument's bits
    // with no INT32 decode at entry; the entry witness checks it in check
    // mode. `PERRY_FACTS_CANONICAL_NUMBERS=0` restores the decode.
    if crate::expr::facts_canonical::enabled() {
        assert!(
            !body.contains(crate::nanbox::INT32_TAG_I64),
            "a proven parameter takes its argument's bits at entry:\n{body}"
        );
    } else {
        assert!(
            body.contains(crate::nanbox::INT32_TAG_I64),
            "a proven parameter converts an INT32 box at entry:\n{body}"
        );
    }
}

#[test]
fn unproven_bindings_keep_the_guarded_lowering() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_with(&[]);
    let body = function_slice(&ir, "perry_fn_facts_num_ts__probe");
    assert!(
        dynamic_calls(body) > 0,
        "unproven operands keep their dynamic arm:\n{body}"
    );
    assert!(
        value_slot_binds(body) > 0,
        "unproven parameters stay rooted:\n{body}"
    );
}

/// One unproven operand keeps `a + b` guarded even when the other operand and
/// the result binding are proven.
#[test]
fn a_partially_proven_operation_stays_guarded() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_with(&[1, 3, 4]);
    let body = function_slice(&ir, "perry_fn_facts_num_ts__probe");
    assert!(
        body.contains("@js_dynamic_string_or_number_add") || body.contains("guarded_add"),
        "`a + b` with an unproven `b` keeps its guard:\n{body}"
    );
}
