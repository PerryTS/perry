//! #10511: the number-local loop clone, read from the emitted IR.
//!
//! The fixture is the SHA-2 shape reduced to one local:
//!
//! ```ts
//! function f(seed: any, n: number) {
//!   let h: any = seed;
//!   for (let i = 0; i < n; i++) { h = ~h ^ i; }
//!   return h;
//! }
//! ```
//!
//! `h` has no function-scope Number proof (its first write is an `any`
//! parameter), so before the tier every `~h` paid the bitwise guard and its
//! cold `js_dynamic_bitnot` arm. Each `declines_*` test is the witness for one
//! admission rule: it goes red when that rule is deleted from the matcher.
//! In particular the two clause tests are the C1 lesson (a proof that walks
//! only the body misses `for (…; …; s = "a")`): sabotage the condition or
//! update walk in `LoopFacts` and they fail.

#![cfg(test)]

use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, CompareOp, Expr, Function, LogicalOp, Module, Param, Stmt, UnaryOp, UpdateOp,
};

use crate::{compile_module, CompileOptions};

const SEED: u32 = 1;
const N: u32 = 2;
const H: u32 = 3;
const I: u32 = 4;

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

/// `h = ~h ^ i`
fn mix_write() -> Stmt {
    Stmt::Expr(Expr::LocalSet(
        H,
        Box::new(bin(
            BinaryOp::BitXor,
            Expr::Unary {
                op: UnaryOp::BitNot,
                operand: Box::new(Expr::LocalGet(H)),
            },
            Expr::LocalGet(I),
        )),
    ))
}

fn counter_let() -> Stmt {
    Stmt::Let {
        id: I,
        name: "i".to_string(),
        ty: Type::Number,
        mutable: true,
        init: Some(Expr::Integer(0)),
    }
}

fn i_lt_n() -> Expr {
    Expr::Compare {
        op: CompareOp::Lt,
        left: Box::new(Expr::LocalGet(I)),
        right: Box::new(Expr::LocalGet(N)),
    }
}

fn i_inc() -> Expr {
    Expr::Update {
        id: I,
        op: UpdateOp::Increment,
        prefix: false,
    }
}

fn module_ir(condition: Expr, update: Expr, body: Vec<Stmt>) -> String {
    let mut module = Module::new("number_local_loop");
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Integer(1), Expr::Integer(8)],
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    module.functions.push(Function {
        id: 1,
        name: "f".to_string(),
        type_params: Vec::new(),
        params: vec![param(SEED, "seed", Type::Any), param(N, "n", Type::Number)],
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: H,
                name: "h".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::LocalGet(SEED)),
            },
            Stmt::For {
                init: Some(Box::new(counter_let())),
                condition: Some(condition),
                update: Some(update),
                body,
            },
            Stmt::Return(Some(Expr::LocalGet(H))),
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
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8")
}

/// The text of the blocks between the first label starting with `from` and
/// the first later label starting with `to`.
fn blocks_between<'a>(ir: &'a str, from: &str, to: &str) -> &'a str {
    let start = ir
        .find(&format!("\n{from}"))
        .unwrap_or_else(|| panic!("premise: no block `{from}*` in\n{ir}"));
    let end = ir[start + 1..]
        .find(&format!("\n{to}"))
        .map_or(ir.len(), |offset| start + 1 + offset);
    &ir[start..end]
}

#[test]
fn admits_a_bitwise_loop_over_an_unproven_local() {
    let ir = module_ir(i_lt_n(), i_inc(), vec![mix_write()]);
    assert!(
        ir.contains("for.number_locals.fast.preheader"),
        "the loop must take the number-local clone\n{ir}"
    );
    // The fast clone: `~h` and `^` are native, no guard and no helper arm.
    let fast = blocks_between(&ir, "for.number_locals_fast", "for.number_locals_slow");
    assert!(
        !fast.contains("@js_dynamic_bitnot") && !fast.contains("@js_dynamic_bitxor"),
        "#10511: the fast clone must not call the dynamic bitwise helpers\n{fast}"
    );
    assert!(
        !fast.contains("guarded_bitnot.dynamic"),
        "#10511: a proven-Number `~h` needs no guard diamond\n{fast}"
    );
    // Premise: the slow clone is the ordinary lowering, which still guards
    // `~h` with the cold helper (so the assertions above are about the clone,
    // not about a lowering that never calls the helper at all).
    let slow = blocks_between(&ir, "for.number_locals_slow", "for.number_locals.merge");
    assert!(
        slow.contains("@js_dynamic_bitnot"),
        "premise: the slow clone keeps the guarded helper arm\n{slow}"
    );
}

#[test]
fn declines_when_the_update_clause_writes_a_non_number() {
    // for (let i = 0; i < n; h = "a") { h = ~h ^ i; i++; }
    let update = Expr::LocalSet(H, Box::new(Expr::String("a".to_string())));
    let body = vec![mix_write(), Stmt::Expr(i_inc())];
    let ir = module_ir(i_lt_n(), update, body);
    assert!(
        !ir.contains("for.number_locals.fast.preheader"),
        "a non-Number write in the UPDATE clause must withdraw `h`\n{ir}"
    );
}

#[test]
fn declines_when_the_condition_writes_a_non_number() {
    // for (let i = 0; (h = "x") && i < n; i++) { h = ~h ^ i; }
    let condition = Expr::Logical {
        op: LogicalOp::And,
        left: Box::new(Expr::LocalSet(H, Box::new(Expr::String("x".to_string())))),
        right: Box::new(i_lt_n()),
    };
    let ir = module_ir(condition, i_inc(), vec![mix_write()]);
    assert!(
        !ir.contains("for.number_locals.fast.preheader"),
        "a non-Number write in the CONDITION must withdraw `h`\n{ir}"
    );
}

#[test]
fn declines_a_body_write_that_may_concatenate() {
    // h = h + "" can produce a string.
    let body = vec![
        mix_write(),
        Stmt::Expr(Expr::LocalSet(
            H,
            Box::new(bin(
                BinaryOp::Add,
                Expr::LocalGet(H),
                Expr::String(String::new()),
            )),
        )),
    ];
    let ir = module_ir(i_lt_n(), i_inc(), body);
    assert!(
        !ir.contains("for.number_locals.fast.preheader"),
        "`h + \"\"` is not Number-producing, so `h` must not be admitted\n{ir}"
    );
}
