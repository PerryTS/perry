//! Rooting and evaluation-order coverage for the `console.*` arms slice 6 of
//! the Layer 1 migration (#7615) repaired — #7649 and its two siblings.
//!
//! # Why unit tests on IR rather than gap tests
//!
//! Three of the four defects here ARE observable from a `.ts` program and one
//! is not, and mixing them in one suite would hide which is which:
//!
//!  * `console.dir(x, y, f())` running `f` after the print, `console.time(l,
//!    f())` never running `f` at all, and `console.table(a, b, c)` printing a
//!    log line instead of a table are all plain evaluation-order / dispatch
//!    facts. They are asserted here on IR *ordering* because that is where the
//!    property is unconditional, and separately A/B'd against node in the PR;
//!  * the rooting half — `data` held in a bare register across `properties`'
//!    lowering — needs `PERRY_GC_MOVING_LOOP_POLLS=1` at **compile** time (off
//!    by default since #7161) plus an arrangement in which the victim's bytes
//!    are actually recycled. A gap test would be green on the default build
//!    whether or not the fix is present, which is CLAUDE.md hazard 4.
//!
//! # What is asserted, and why it cannot pass vacuously
//!
//! Never a slot count for the rooting arms: counting root slots across two
//! programs that differ in an operand lets the *other* operand's rooting pay
//! for the assertion. What is asserted is the ordering that IS the bug — the
//! register the consuming call reads must be defined BELOW the allocation it
//! must survive, which can only happen if the value was rooted above the window
//! and re-read below it. Each test first asserts, by callee name, that the arm
//! under test was reached at all.
//!
//! The zero-cost arm is the counterpart: a single-argument `console.table`
//! cannot collect between its operand and the call, so `operand_protection`
//! must route it to `Reuse` and emit no temp-root traffic. Without that, a
//! future "root everything" change would tax every `console.table(rows)` in
//! every program and no test would notice.

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

/// `console.<method>(args…)`.
///
/// The receiver must be a `GlobalGet` — that is what `try_lower_console_call`
/// dispatches on — and it is never lowered by these arms, so any id serves.
fn console_call(method: &str, args: Vec<Expr>) -> Vec<Stmt> {
    vec![Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::GlobalGet(0)),
            property: method.to_string(),
            byte_offset: 0,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    })]
}

/// A heap value whose lowering allocates, so the window it sits in collects.
///
/// An object literal rather than a call: codegen does not type-check the
/// argument, `operand_needs_root` protects it exactly as it protects any other
/// heap operand, and it keeps the HIR small enough to read.
fn allocating(tag: &str) -> Expr {
    Expr::Object(vec![(tag.to_string(), Expr::Number(1.0))])
}

/// Line index of a **call** to `callee`, or `None`.
///
/// Excluding `declare` is load-bearing rather than tidy: the module carries a
/// `declare` for the helper whether or not anything calls it, so a liveness
/// check that accepted it would be satisfied by a lowering that never ran —
/// hazard 4 wearing the subject's name.
fn call_line(ir: &str, callee: &str) -> Option<usize> {
    let needle = format!("@{callee}(");
    ir.lines()
        .position(|l| l.contains(&needle) && !l.trim_start().starts_with("declare"))
}

fn require_call_line(ir: &str, callee: &str) -> usize {
    call_line(ir, callee).unwrap_or_else(|| panic!("no call to {callee} in:\n{ir}"))
}

/// The first `double` SSA operand of the call to `callee`.
fn first_double_operand(ir: &str, callee: &str) -> String {
    let idx = require_call_line(ir, callee);
    let line = ir.lines().nth(idx).expect("line index came from this IR");
    line.split("(double ")
        .nth(1)
        .unwrap_or_else(|| panic!("{callee} takes a double first argument: {line}"))
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

/// Line index of the LAST allocation emitted before `before`.
fn last_alloc_before(ir: &str, before: usize) -> usize {
    ir.lines()
        .enumerate()
        .take(before)
        .filter(|(_, l)| l.contains("@js_object_alloc"))
        .map(|(i, _)| i)
        .last()
        .unwrap_or_else(|| panic!("no object allocation above line {before} in:\n{ir}"))
}

/// Temp-root traffic, excluding the `declare` lines that name the helpers
/// whether or not anything calls them.
fn temp_root_calls(ir: &str) -> usize {
    ir.lines()
        .filter(|l| !l.trim_start().starts_with("declare"))
        .filter(|l| l.contains("js_gc_temp_root"))
        .count()
}

/// #7649: `console.table(data, properties)` held `data` in a bare register
/// across `properties`' lowering.
#[test]
fn console_table_roots_its_data_across_the_properties_operand() {
    let ir = compile_body(
        "table_hot",
        console_call("table", vec![allocating("data"), allocating("props")]),
    );

    let call = require_call_line(&ir, "js_console_table_with_properties");
    let data = first_double_operand(&ir, "js_console_table_with_properties");
    let data_def = definition_line(&ir, &data);
    let props_alloc = last_alloc_before(&ir, call);
    assert!(
        data_def > props_alloc,
        "console.table's `data` register {data} is defined at line {data_def}, ABOVE the \
         `properties` allocation at line {props_alloc}. That is the unrooted window: an \
         evacuating minor while `properties` is evaluated relocates `data` and the renderer \
         then reads from-space. `data` must be rooted above the window and re-read below it.\n{ir}"
    );
}

/// The zero-cost counterpart: nothing follows a lone `data` operand, so the
/// protection decision must be `Reuse` and the emission unchanged.
#[test]
fn console_table_with_one_argument_emits_no_rooting_traffic() {
    let ir = compile_body(
        "table_cold",
        console_call("table", vec![allocating("data")]),
    );

    require_call_line(&ir, "js_console_table");
    assert_eq!(
        temp_root_calls(&ir),
        0,
        "a single-argument console.table cannot collect between its operand and the call, so \
         operand_protection must route it to Reuse. Rooting unconditionally taxes every \
         `console.table(rows)` in every program.\n{ir}"
    );
}

/// #7649's arity half: three or more arguments used to fall through to the
/// generic multi-argument `console.log` arm and print the array.
#[test]
fn console_table_still_renders_a_table_with_surplus_arguments() {
    let ir = compile_body(
        "table_surplus",
        console_call(
            "table",
            vec![allocating("data"), allocating("props"), allocating("extra")],
        ),
    );

    assert!(
        call_line(&ir, "js_console_table_with_properties").is_some(),
        "console.table(a, b, c) must still render a table; node ignores the surplus arguments \
         rather than stopping being console.table\n{ir}"
    );
    assert!(
        call_line(&ir, "js_console_log_spread").is_none(),
        "console.table(a, b, c) fell through to the generic multi-arg console.log arm\n{ir}"
    );
}

/// #7649's evaluation-order half: `console.dir` lowered `args[2..]` AFTER the
/// call that produces the print, so `console.dir(x, y, f())` ran `f` second.
/// Node evaluates every argument before invoking anything.
#[test]
fn console_dir_evaluates_surplus_arguments_before_it_prints() {
    let ir = compile_body(
        "dir_order",
        console_call(
            "dir",
            vec![
                allocating("obj"),
                allocating("opts"),
                allocating("side_effect"),
            ],
        ),
    );

    let call = require_call_line(&ir, "js_console_dir_with_options");
    let allocs_below = ir
        .lines()
        .skip(call)
        .filter(|l| l.contains("@js_object_alloc"))
        .count();
    assert_eq!(
        allocs_below, 0,
        "console.dir lowered a surplus argument below its own print at line {call}. Node \
         evaluates the whole argument list before it invokes the callee, so a side effect in \
         argument 3 must run FIRST.\n{ir}"
    );
}

/// The same defect one arm over, with the surplus argument dropped entirely
/// rather than resequenced: `console.time(label, f())` never lowered `f`.
#[test]
fn console_time_evaluates_its_surplus_arguments() {
    // Differential rather than absolute. One object literal lowers to several
    // `js_object_alloc*` lines (an inline-bump fast path, a slow path and the
    // shape declaration), so a fixed expected count would be pinning an
    // unrelated lowering detail. What must hold is that adding an argument adds
    // its evaluation.
    let allocs_above_call = |ir: &str| {
        let call = require_call_line(ir, "js_console_time_value");
        ir.lines()
            .take(call)
            .filter(|l| l.contains("@js_object_alloc"))
            .count()
    };

    let label_only = compile_body(
        "time_label",
        console_call("time", vec![allocating("label")]),
    );
    let with_surplus = compile_body(
        "time_order",
        console_call("time", vec![allocating("label"), allocating("side_effect")]),
    );

    assert!(
        allocs_above_call(&with_surplus) > allocs_above_call(&label_only),
        "console.time uses only the label, but node EVALUATES every argument. \
         `console.time(l, sideEffect())` emitted the same allocations as `console.time(l)`, \
         which means the surplus argument was never lowered and its side effect never \
         happened.\n{with_surplus}"
    );
}
