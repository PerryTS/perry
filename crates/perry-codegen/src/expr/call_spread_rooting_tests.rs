//! Rooting coverage for the spread argument-bundle accumulator (#7664).
//!
//! # What is asserted, and why it cannot pass vacuously
//!
//! **Ordering, never slot counts.** The harness is slice 7's and the reason is
//! slice 8's, restated because it is the trap this file would otherwise fall
//! into: on the DEFAULT build `reserve_shadow_slot` hands back a stack-map
//! index, no `js_shadow_frame_enter` is emitted, and **no `js_gc_temp_root_*`
//! call is emitted either** — the pooled-alloca lowering is a plain
//! `store`/`load` pair. So `temp_root_calls(ir) > 0` reads **zero** here and a
//! test built on it would assert nothing at all. The property that is visible
//! in all three lowerings (pooled alloca, shadow frame, FFI fallback) is
//! **where the accumulator operand is defined**: below the allocating call, or
//! above it.
//!
//! **Every fold is checked, not the first one.** `require_call_line` returns
//! the first match; a bundle emits one `js_array_concat` per spread source, and
//! checking only the first would leave a partially-fixed loop green. [`assert_every_fold_rereads_the_accumulator`]
//! walks them all.
//!
//! # The window, and why it is never empty
//!
//! `js_array_like_to_array` allocates unconditionally. An `Expr::CallSpread`
//! has at least one spread source by construction, so every bundle has at least
//! one allocating call between `js_array_alloc` and the `js_array_concat` that
//! consumes the accumulator. That is why these tests use INERT operands
//! (`Expr::Number`) in one arm: the hazard does not need a user callback to
//! exist, and an operands-only "can this collect?" predicate answers `false`
//! for `f(...[1, 2])` while the accumulator is still at risk.

use perry_hir::types::Type;
use perry_hir::{CallArg, Expr, Function, Module as HirModule, Stmt};

use super::slice7_rooting_tests::allocating;

/// Compile a one-function module whose body is `stmts` and return its LLVM IR.
///
/// A local copy rather than a re-export: `compile_body` is private to slice 7
/// and duplicating six fields is cheaper than widening its visibility for one
/// caller.
fn compile_body(name: &str, body: Vec<Stmt>) -> String {
    let mut hir = HirModule::new(name);
    hir.functions.push(Function {
        id: 0,
        name: "build".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
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
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let bytes = crate::compile_module(&hir, opts).expect("test module compiles");
    String::from_utf8(bytes).expect("LLVM IR is UTF-8")
}

/// Line indices of every non-`declare` call to `callee`.
fn call_lines(ir: &str, callee: &str) -> Vec<usize> {
    let needle = format!("@{callee}(");
    ir.lines()
        .enumerate()
        .filter(|(_, l)| l.contains(&needle) && !l.trim_start().starts_with("declare"))
        .map(|(i, _)| i)
        .collect()
}

/// The register passed as argument 0 of the call on `line`.
fn first_operand(ir: &str, line: usize) -> String {
    let text = ir.lines().nth(line).expect("line index came from this IR");
    let args = text
        .rsplit_once('(')
        .unwrap_or_else(|| panic!("call has no argument list: {text}"))
        .1;
    args.split(',')
        .next()
        .expect("a call has at least one argument")
        .trim()
        .rsplit(' ')
        .next()
        .expect("an operand is a type followed by a register")
        .trim_end_matches(')')
        .to_string()
}

fn definition_line(ir: &str, reg: &str) -> Option<usize> {
    let prefix = format!("{reg} = ");
    ir.lines().position(|l| l.trim_start().starts_with(&prefix))
}

/// Every `js_array_concat` in `ir` must read an accumulator register defined
/// BELOW the `js_array_like_to_array` immediately above it.
///
/// That is the whole invariant. `js_array_like_to_array` allocates; a register
/// defined above it and used below it is the `unrooted:alloc` hazard
/// `--statepoints --moving-only` reports, because nothing in its cast chain
/// appears in that safepoint's live bundle.
fn assert_every_fold_rereads_the_accumulator(ir: &str, what: &str) {
    let folds = call_lines(ir, "js_array_concat");
    assert!(
        !folds.is_empty(),
        "{what}: no js_array_concat in the emitted IR, so this test asserts \
         nothing about the bundle it was written for. The lowering changed \
         shape — re-aim the test rather than deleting it.\n{ir}"
    );
    let windows = call_lines(ir, "js_array_like_to_array");
    assert!(
        !windows.is_empty(),
        "{what}: no js_array_like_to_array, so there is no window and the \
         assertion below is vacuous.\n{ir}"
    );
    for fold in folds {
        let acc = first_operand(ir, fold);
        let def = definition_line(ir, &acc)
            .unwrap_or_else(|| panic!("{what}: no definition for the accumulator {acc} in:\n{ir}"));
        let window = windows
            .iter()
            .copied()
            .filter(|&w| w < fold)
            .next_back()
            .unwrap_or_else(|| {
                panic!("{what}: js_array_concat at line {fold} has no allocating conversion above it\n{ir}")
            });
        assert!(
            def > window,
            "{what}: js_array_concat at line {fold} reads {acc}, defined at line {def}, \
             ABOVE the js_array_like_to_array at line {window}. That conversion allocates, \
             so an evacuating minor inside it relocates the half-built argument array and \
             this concat reads from-space. The accumulator must be rooted above the window \
             and re-read below it.\n{ir}"
        );
    }
}

/// `cb(...spread, regular)` — a regular argument AFTER a spread, which forces
/// the source-ordered single-array path (`interleaved`).
#[test]
fn an_interleaved_spread_bundle_rereads_its_accumulator_below_the_array_conversion() {
    let ir = compile_body(
        "interleaved_spread",
        vec![Stmt::Expr(Expr::CallSpread {
            callee: Box::new(allocating("cb")),
            args: vec![
                CallArg::Spread(allocating("a")),
                CallArg::Expr(allocating("b")),
                CallArg::Spread(allocating("c")),
            ],
            type_args: Vec::new(),
        })],
    );
    assert_every_fold_rereads_the_accumulator(&ir, "interleaved spread bundle");
}

/// `cb(...a, ...b)` — two spread sources and no regular argument, which takes
/// the multi-spread marshalling arm instead.
#[test]
fn a_multi_spread_bundle_rereads_its_accumulator_below_the_array_conversion() {
    let ir = compile_body(
        "multi_spread",
        vec![Stmt::Expr(Expr::CallSpread {
            callee: Box::new(allocating("cb")),
            args: vec![
                CallArg::Spread(allocating("a")),
                CallArg::Spread(allocating("b")),
            ],
            type_args: Vec::new(),
        })],
    );
    assert_every_fold_rereads_the_accumulator(&ir, "multi-spread bundle");
}

/// ★ The operands are INERT — `[1, 2]` and `3` cannot run user code — and the
/// accumulator still has to be rooted, because `js_array_like_to_array` itself
/// allocates.
///
/// This is the arm that pins the `protect` predicate. An operands-only "can
/// anything here collect?" test answers `false` for this program, which is the
/// answer `rooting::any_operand_may_collect` gives on its own; the bundle
/// disjoins the spread-present test precisely so this case still roots.
#[test]
fn an_all_inert_spread_bundle_still_roots_its_accumulator() {
    let ir = compile_body(
        "inert_spread",
        vec![Stmt::Expr(Expr::CallSpread {
            callee: Box::new(allocating("cb")),
            args: vec![
                CallArg::Spread(Expr::Array(vec![Expr::Number(1.0), Expr::Number(2.0)])),
                CallArg::Expr(Expr::Number(3.0)),
                CallArg::Spread(Expr::Array(vec![Expr::Number(4.0)])),
            ],
            type_args: Vec::new(),
        })],
    );
    assert_every_fold_rereads_the_accumulator(&ir, "all-inert spread bundle");
}
