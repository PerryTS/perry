//! Rooting coverage for the two-argument `setTimeout` / `setInterval` arms
//! repaired by slice 5 of the Layer 1 migration (#7615).
//!
//! # Why a unit test rather than a gap test
//!
//! The bug needs three things to fault at runtime: a **capturing** callback (a
//! non-capturing arrow lowers to `js_closure_alloc_singleton`, which is never
//! moved, so the stale register keeps working), a delay expression that reaches
//! a back-edge poll, and `PERRY_GC_MOVING_LOOP_POLLS=1` at *compile* time — off
//! by default since #7161. A gap test would therefore be green on the default
//! build regardless of whether the fix is present, which is CLAUDE.md hazard 4:
//! a gate whose subject never ran.
//!
//! These assert on the emitted IR instead, where the property is unconditional.
//!
//! # What is asserted, and why it cannot pass vacuously
//!
//! Not a slot COUNT. Counting root slots across two programs that differ in an
//! operand would let the delay expression's own rooting pay for the assertion —
//! the test would stay green with the repair deleted. What is asserted is the
//! ordering that IS the bug:
//!
//!  * **hot** (an allocating delay): the register `js_timer_validate_callback`
//!    reads must be defined **below** the delay's last allocation. That can only
//!    happen if the callback was rooted above the window and re-read below it.
//!    With the repair removed the callback's register is defined above the
//!    delay and the assertion fails.
//!  * **cold** (a literal delay): the same register must be defined **above**
//!    the consuming call with **no** temp-root traffic anywhere in the module.
//!    This pins the zero-cost claim — `operand_protection` routes a
//!    non-collecting window to `Reuse` — so a future change that roots
//!    unconditionally turns this red rather than silently taxing every
//!    `setTimeout(fn, 100)` in every program.
//!
//! Both arms first assert the arm under test was actually reached, by callee
//! name, so an IR shape measured over a lowering that never ran cannot pass.

use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module as HirModule, Stmt};

/// Compile a one-function module and return its LLVM IR.
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

/// `timer(callback, delay)` as an `ExternFuncRef` call — the shape
/// `try_lower_extern_timer_call` dispatches on.
///
/// The callback is an object literal rather than a closure on purpose: codegen
/// does not type-check it, an object literal is a heap value that
/// `operand_needs_root` protects exactly as a capturing closure is, and it keeps
/// the HIR small enough to read.
fn timer_call(timer: &str, delay: Expr) -> Vec<Stmt> {
    vec![Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: timer.to_string(),
            param_types: vec![Type::Any, Type::Number],
            return_type: Type::Any,
        }),
        args: vec![
            Expr::Object(vec![("cb".to_string(), Expr::Number(1.0))]),
            delay,
        ],
        type_args: Vec::new(),
        byte_offset: 0,
    })]
}

/// A delay that allocates (so the window collects), and one that cannot.
fn allocating_delay() -> Expr {
    Expr::Object(vec![("d".to_string(), Expr::Number(1.0))])
}

fn inert_delay() -> Expr {
    Expr::Number(5.0)
}

/// Line index of the `js_timer_validate_callback` **call**, or `None`.
///
/// Excluding `declare` is load-bearing rather than tidy: the module always
/// carries `declare i64 @js_timer_validate_callback(double, i32)` whether or not
/// anything calls it, so a liveness check that accepted it would be satisfied by
/// a lowering that never ran — hazard 4 wearing the subject's name.
fn validate_call_line(ir: &str) -> Option<usize> {
    ir.lines().position(|l| {
        l.contains("@js_timer_validate_callback(") && !l.trim_start().starts_with("declare")
    })
}

/// The SSA register `js_timer_validate_callback` actually reads.
fn callback_operand(ir: &str) -> String {
    let idx = validate_call_line(ir).expect("no js_timer_validate_callback call");
    let line = ir.lines().nth(idx).expect("line index came from this IR");
    let after = line
        .split("(double ")
        .nth(1)
        .expect("js_timer_validate_callback takes a double first argument");
    after
        .split([',', ')'])
        .next()
        .expect("the first argument is comma- or paren-terminated")
        .trim()
        .to_string()
}

/// Line index at which `reg` is defined.
fn definition_line(ir: &str, reg: &str) -> usize {
    let prefix = format!("{reg} = ");
    ir.lines()
        .position(|l| l.trim_start().starts_with(&prefix))
        .unwrap_or_else(|| panic!("no definition for {reg} in:\n{ir}"))
}

/// Line index of the LAST occurrence of `needle`.
fn last_line_containing(ir: &str, needle: &str) -> usize {
    ir.lines()
        .enumerate()
        .filter(|(_, l)| l.contains(needle))
        .map(|(i, _)| i)
        .last()
        .unwrap_or_else(|| panic!("no line containing {needle} in:\n{ir}"))
}

/// An allocating delay must leave the callback re-read BELOW the allocation.
fn assert_callback_survives_an_allocating_delay(timer: &str) {
    let ir = compile_body(
        &format!("{timer}_hot"),
        timer_call(timer, allocating_delay()),
    );

    // Subject liveness first: an IR shape measured over a lowering that never
    // ran proves nothing (CLAUDE.md hazard 4).
    assert!(
        validate_call_line(&ir).is_some(),
        "{timer}: the two-argument arm was not reached:\n{ir}"
    );

    let operand = callback_operand(&ir);
    let operand_def = definition_line(&ir, &operand);
    // The delay is the SECOND object literal, so its allocation is the last one
    // emitted before the timer call.
    let delay_alloc = last_line_containing(&ir, "@js_object_alloc");
    assert!(
        operand_def > delay_alloc,
        "{timer}: the callback register {operand} is defined at line {operand_def}, ABOVE the \
         delay's allocation at line {delay_alloc}. That is the unrooted window: an evacuating \
         minor inside the delay relocates the callback and js_timer_validate_callback then reads \
         from-space. The callback must be rooted above the window and re-read below it.\n{ir}"
    );
}

/// A literal delay cannot collect, so the repair must emit nothing at all.
fn assert_inert_delay_costs_nothing(timer: &str) {
    let ir = compile_body(&format!("{timer}_cold"), timer_call(timer, inert_delay()));

    let validate = validate_call_line(&ir)
        .unwrap_or_else(|| panic!("{timer}: the two-argument arm was not reached:\n{ir}"));

    let operand = callback_operand(&ir);
    let operand_def = definition_line(&ir, &operand);
    assert!(
        operand_def < validate,
        "{timer}: the callback operand must be the register the callback was lowered into"
    );

    // Temp-root traffic only ever appears as a call; `declare` lines name the
    // helpers whether or not anything calls them, so they are excluded.
    let pushes = ir
        .lines()
        .filter(|l| !l.trim_start().starts_with("declare"))
        .filter(|l| l.contains("js_gc_temp_root"))
        .count();
    assert_eq!(
        pushes, 0,
        "{timer}: a literal delay cannot collect, so operand_protection must route the callback \
         to Reuse and emit no temp-root traffic. Rooting it unconditionally taxes every \
         `{timer}(fn, 100)` in every program.\n{ir}"
    );
}

#[test]
fn set_timeout_two_arg_roots_the_callback_across_an_allocating_delay() {
    assert_callback_survives_an_allocating_delay("setTimeout");
}

#[test]
fn set_interval_two_arg_roots_the_callback_across_an_allocating_delay() {
    assert_callback_survives_an_allocating_delay("setInterval");
}

#[test]
fn set_timeout_two_arg_costs_nothing_when_the_delay_is_a_literal() {
    assert_inert_delay_costs_nothing("setTimeout");
}

#[test]
fn set_interval_two_arg_costs_nothing_when_the_delay_is_a_literal() {
    assert_inert_delay_costs_nothing("setInterval");
}
