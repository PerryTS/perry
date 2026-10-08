//! #10718: counter-offset stores in the DENSE range-loop mode, read from the
//! emitted IR.
//!
//! The fixture is the issue's read-modify-write row:
//!
//! ```ts
//! function f(a: number[], x: any) {
//!   let s = 0;
//!   for (let i = 0; i < 400; i++) { a[i] = a[i] + 1; s = s + a[i]; }
//!   return s;
//! }
//! ```
//!
//! Two statements, so the classic (single-statement) mode cannot take it, and
//! the dense mode admitted only masked stores — the loop paid ~206
//! instructions per element on the generic path. Dense mode's guarantee is
//! that an iteration runs entirely in one copy, which is what makes a store
//! legal in a multi-statement body; the price is that the store may have NO
//! side exit, so its value must be a statically genuine double.
//! `declines_a_store_whose_value_is_not_provably_a_double` is the witness for
//! that rule: delete the RHS check from the dense counter-store arm and it
//! goes red (verified by sabotage) — the store would need a value check whose
//! failure has nowhere safe to go.

#![cfg(test)]

use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

use super::loops::packed_f64_range_loop_compound_alias_fold_all;
use crate::{compile_module, CompileOptions};

const A: u32 = 1;
const X: u32 = 2;
const S: u32 = 3;
const I: u32 = 4;
const BASE: u32 = 5;
const KEY: u32 = 6;

fn param(id: u32, name: &str, ty: Type) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
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

fn get(arr: u32, idx: u32) -> Expr {
    Expr::IndexGet {
        object: Box::new(Expr::LocalGet(arr)),
        index: Box::new(Expr::LocalGet(idx)),
    }
}

fn set(arr: u32, idx: u32, value: Expr) -> Stmt {
    Stmt::Expr(Expr::IndexSet {
        object: Box::new(Expr::LocalGet(arr)),
        index: Box::new(Expr::LocalGet(idx)),
        value: Box::new(value),
    })
}

/// `s = s + a[i]`
fn accumulate() -> Stmt {
    Stmt::Expr(Expr::LocalSet(
        S,
        Box::new(bin(BinaryOp::Add, Expr::LocalGet(S), get(A, I))),
    ))
}

fn alias(id: u32, name: &str, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Number,
        mutable: false,
        init: Some(init),
    }
}

fn module_ir(body: Vec<Stmt>) -> String {
    let mut module = Module::new("dense_rmw");
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Array(vec![Expr::Number(0.5)]), Expr::Integer(1)],
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    module.functions.push(Function {
        id: 1,
        name: "f".to_string(),
        type_params: Vec::new(),
        params: vec![
            param(A, "a", Type::Array(Box::new(Type::Number))),
            param(X, "x", Type::Any),
        ],
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: S,
                name: "s".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: I,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(I)),
                    right: Box::new(Expr::Integer(400)),
                }),
                update: Some(Expr::Update {
                    id: I,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body,
            },
            Stmt::Return(Some(Expr::LocalGet(S))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        disable_constfn_shapes: false,
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8")
}

const DENSE_GUARD: &str = "@js_typed_feedback_packed_f64_range_loop_guard_dense(";

/// Whether the IR CALLS `callee`. A bare `contains` would also match the
/// module's `declare` line, which every module carries — a check that cannot
/// fail.
fn calls(ir: &str, callee: &str) -> bool {
    ir.lines()
        .any(|line| line.contains(" call ") && line.contains(callee))
}

#[test]
fn a_read_modify_write_body_takes_the_dense_tier() {
    // a[i] = a[i] + 1; s = s + a[i];
    let body = vec![
        set(A, I, bin(BinaryOp::Add, get(A, I), Expr::Integer(1))),
        accumulate(),
    ];
    let ir = module_ir(body);
    assert!(
        calls(&ir, DENSE_GUARD),
        "#10718: the two-statement RMW body must take the dense range tier\n{ir}"
    );
    assert!(
        !ir.contains("packed_f64_range_store.side_exit"),
        "#10718: a dense store has no side exit — its value is proven a double\n{ir}"
    );
}

#[test]
fn the_compound_spelling_folds_and_takes_the_dense_tier() {
    // a[i] += 1; s = s + a[i];   (HIR: two alias lets, the store, the sum)
    let body = vec![
        alias(BASE, "__cmpd_base_5", Expr::LocalGet(A)),
        alias(KEY, "__cmpd_key_6", Expr::LocalGet(I)),
        set(
            BASE,
            KEY,
            bin(BinaryOp::Add, get(BASE, KEY), Expr::Integer(1)),
        ),
        accumulate(),
    ];
    let ir = module_ir(body);
    assert!(
        calls(&ir, DENSE_GUARD),
        "#10718: `a[i] += 1; s += a[i]` must fold and take the dense tier\n{ir}"
    );
}

#[test]
fn declines_a_store_whose_value_is_not_provably_a_double() {
    // a[i] = undefined; s = s + a[i];
    // `undefined` passes the non-collecting walk (evaluating it runs no
    // code), so the ONLY gate between it and a raw `store double` of its
    // NaN-box tag into a raw-f64 slot is the statically-genuine RHS rule.
    let body = vec![set(A, I, Expr::Undefined), accumulate()];
    let ir = module_ir(body);
    assert!(
        !calls(&ir, DENSE_GUARD),
        "#10718: a dense store needs a statically genuine double RHS\n{ir}"
    );
}

#[test]
fn declines_a_store_that_may_concatenate() {
    // a[i] = a[i] + x; s = s + a[i];  with `x: any`.
    let body = vec![
        set(A, I, bin(BinaryOp::Add, get(A, I), Expr::LocalGet(X))),
        accumulate(),
    ];
    let ir = module_ir(body);
    assert!(
        !calls(&ir, DENSE_GUARD),
        "#10718: `a[i] + x` may concatenate; it is no dense store value\n{ir}"
    );
}

#[test]
fn fold_all_declines_an_alias_read_by_a_later_statement() {
    // The alias must be read only by the statement it was minted for: a later
    // statement that read it would read a slot the fast clone never writes.
    let body = vec![
        alias(BASE, "__cmpd_base_5", Expr::LocalGet(A)),
        alias(KEY, "__cmpd_key_6", Expr::LocalGet(I)),
        set(
            BASE,
            KEY,
            bin(BinaryOp::Add, get(BASE, KEY), Expr::Integer(1)),
        ),
        Stmt::Expr(Expr::LocalSet(
            S,
            Box::new(bin(BinaryOp::Add, Expr::LocalGet(S), get(BASE, KEY))),
        )),
    ];
    assert!(
        packed_f64_range_loop_compound_alias_fold_all(&body).is_none(),
        "an alias read after its statement must refuse the fold"
    );
}

#[test]
fn fold_all_folds_every_alias_run() {
    let run = |base: u32, key: u32| {
        vec![
            alias(base, &format!("__cmpd_base_{base}"), Expr::LocalGet(A)),
            alias(key, &format!("__cmpd_key_{key}"), Expr::LocalGet(I)),
            set(
                base,
                key,
                bin(BinaryOp::Add, get(base, key), Expr::Integer(1)),
            ),
        ]
    };
    let mut body = run(BASE, KEY);
    body.push(accumulate());
    body.extend(run(7, 8));
    let folded = packed_f64_range_loop_compound_alias_fold_all(&body).expect("both runs must fold");
    assert_eq!(folded.len(), 3, "two folded stores and the sum: {folded:?}");
    let text = format!("{folded:?}");
    for id in [BASE, KEY, 7, 8] {
        assert!(
            !text.contains(&format!("LocalGet({id})")),
            "alias {id} survived the fold: {text}"
        );
    }
}
