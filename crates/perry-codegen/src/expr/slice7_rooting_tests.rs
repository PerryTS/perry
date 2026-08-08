//! Rooting and evaluation-order coverage for the three `expr/` modules slice 7
//! of the Layer 1 migration (#7615) repaired.
//!
//! # What is asserted, and why it cannot pass vacuously
//!
//! **Ordering, never slot counts.** A count lets one operand's rooting pay for
//! another's assertion; the property that IS the bug is that the register a
//! consuming call reads must be defined BELOW the allocation it has to survive,
//! which can only happen if the value was rooted above the window and re-read
//! below it. Every test first asserts, by callee name, that the arm under test
//! was reached at all — a shape measured over a lowering that never ran is
//! CLAUDE.md hazard 4 wearing the subject's name.
//!
//! **Raw pointers get their own tests.** Half of what this slice fixed is
//! #7280 taxonomy (a) — a value stripped to a bare heap pointer ABOVE a
//! collection point. `root_reload` structurally cannot repair those, so the
//! assertion is on the `and i64 …, POINTER_MASK` (or the
//! `js_jsvalue_to_string_coerce` call) rather than on the boxed operand.
//!
//! **The zero-cost arm is a test too.** A single-operand `Reflect.ownKeys` and
//! a no-options `execSync` cannot collect between their operand and the call,
//! so `operand_protection` must route them to `Reuse` and emit no temp-root
//! traffic at all. Without that pin a future "root everything" change would tax
//! every call site and nothing would notice.

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

/// A heap value whose lowering allocates, so the window it sits in collects.
fn allocating(tag: &str) -> Expr {
    Expr::Object(vec![(tag.to_string(), Expr::Number(1.0))])
}

/// Line index of a **call** to `callee`, or `None`.
///
/// Excluding `declare` is load-bearing rather than tidy: the module carries a
/// `declare` for the helper whether or not anything calls it, so a liveness
/// check that accepted it would be satisfied by a lowering that never ran.
fn call_line(ir: &str, callee: &str) -> Option<usize> {
    let needle = format!("@{callee}(");
    ir.lines()
        .position(|l| l.contains(&needle) && !l.trim_start().starts_with("declare"))
}

fn require_call_line(ir: &str, callee: &str) -> usize {
    call_line(ir, callee).unwrap_or_else(|| panic!("no call to {callee} in:\n{ir}"))
}

/// The `n`-th SSA operand of the call to `callee`, whatever its type.
fn call_operand(ir: &str, callee: &str, n: usize) -> String {
    let idx = require_call_line(ir, callee);
    let line = ir.lines().nth(idx).expect("line index came from this IR");
    let args = line
        .rsplit_once('(')
        .unwrap_or_else(|| panic!("{callee} call has no argument list: {line}"))
        .1;
    args.split(',')
        .nth(n)
        .unwrap_or_else(|| panic!("{callee} has no operand {n}: {line}"))
        .trim()
        .rsplit(' ')
        .next()
        .expect("an operand is a type followed by a register")
        .trim_end_matches(')')
        .to_string()
}

/// Line index at which `reg` is defined, or `None` for a literal / constant.
fn definition_line(ir: &str, reg: &str) -> Option<usize> {
    let prefix = format!("{reg} = ");
    ir.lines().position(|l| l.trim_start().starts_with(&prefix))
}

fn require_definition_line(ir: &str, reg: &str) -> usize {
    definition_line(ir, reg).unwrap_or_else(|| panic!("no definition for {reg} in:\n{ir}"))
}

/// Line index of the LAST object allocation emitted before `before`.
fn last_alloc_before(ir: &str, before: usize) -> usize {
    ir.lines()
        .enumerate()
        .take(before)
        .filter(|(_, l)| l.contains("@js_object_alloc"))
        .map(|(i, _)| i)
        .last()
        .unwrap_or_else(|| panic!("no object allocation above line {before} in:\n{ir}"))
}

/// Line index of the last object allocation anywhere in the module.
fn last_alloc(ir: &str) -> usize {
    last_alloc_before(ir, ir.lines().count())
}

/// Temp-root traffic, excluding the `declare` lines that name the helpers
/// whether or not anything calls them.
fn temp_root_calls(ir: &str) -> usize {
    ir.lines()
        .filter(|l| !l.trim_start().starts_with("declare"))
        .filter(|l| l.contains("js_gc_temp_root"))
        .count()
}

/// Assert that the register `callee` reads as operand `n` is defined below the
/// last allocation above the call — i.e. that the value was re-read after the
/// window rather than carried across it.
fn assert_operand_survives_the_window(ir: &str, callee: &str, n: usize, what: &str) {
    let call = require_call_line(ir, callee);
    let reg = call_operand(ir, callee, n);
    let def = require_definition_line(ir, &reg);
    let alloc = last_alloc_before(ir, call);
    assert!(
        def > alloc,
        "{what}: {callee} reads {reg}, defined at line {def}, ABOVE the allocation at line \
         {alloc}. That is the unrooted window — an evacuating minor there relocates the value \
         and the helper dereferences from-space. It must be rooted above the window and \
         re-read below it.\n{ir}"
    );
}

// ---------------------------------------------------------------------------
// expr/child_proc.rs
// ---------------------------------------------------------------------------

/// The raw command pointer used to be stripped out of `cmd_box` ABOVE the
/// options lowering — taxonomy (a), unrepairable by any re-read.
#[test]
fn exec_sync_strips_its_command_pointer_below_the_options_operand() {
    let ir = compile_body(
        "exec_sync_window",
        vec![Stmt::Expr(Expr::ChildProcessExecSync {
            command: Box::new(Expr::String("ls".to_string())),
            options: Some(Box::new(allocating("opts"))),
        })],
    );
    assert_operand_survives_the_window(
        &ir,
        "js_child_process_exec_sync",
        0,
        "execSync's command is a heap string and `options` is arbitrary user code",
    );
}

/// The zero-cost counterpart: with no options nothing follows the command, so
/// `operand_protection` must answer `Reuse` and emit nothing.
#[test]
fn exec_sync_without_options_emits_no_rooting_traffic() {
    let ir = compile_body(
        "exec_sync_cold",
        vec![Stmt::Expr(Expr::ChildProcessExecSync {
            command: Box::new(Expr::String("ls".to_string())),
            options: None,
        })],
    );
    require_call_line(&ir, "js_child_process_exec_sync");
    assert_eq!(
        temp_root_calls(&ir),
        0,
        "a no-options execSync cannot collect between its operand and the call, so \
         operand_protection must route it to Reuse\n{ir}"
    );
}

/// Evaluation order: node evaluates a call's whole argument list before the
/// callee is entered, so `spawnSync(badFile, sideEffect())` runs the side
/// effect and only then throws. Perry validated `command` between the two
/// lowerings.
#[test]
fn spawn_sync_validates_below_the_whole_argument_list() {
    let ir = compile_body(
        "spawn_sync_order",
        vec![Stmt::Expr(Expr::ChildProcessSpawnSync {
            command: Box::new(Expr::String("ls".to_string())),
            args: Some(Box::new(allocating("args"))),
            options: Some(Box::new(allocating("opts"))),
        })],
    );
    let validate = require_call_line(&ir, "js_child_process_validate_command");
    let last_operand = last_alloc(&ir);
    assert!(
        validate > last_operand,
        "js_child_process_validate_command is emitted at line {validate}, ABOVE the last \
         argument's allocation at line {last_operand}. A throw there skips evaluation of the \
         later arguments, which JS performs before the callee is entered.\n{ir}"
    );
}

/// `fork` coerced the module path to a RAW `StringHeader*` and then held it
/// across `args`' and `options`' lowering — #7453's shape at a second site.
#[test]
fn fork_coerces_its_module_path_below_every_other_operand() {
    let ir = compile_body(
        "fork_window",
        vec![Stmt::Expr(Expr::ChildProcessFork {
            module: Box::new(Expr::String("./w.js".to_string())),
            args: Some(Box::new(allocating("args"))),
            options: Some(Box::new(allocating("opts"))),
        })],
    );
    let coerce = require_call_line(&ir, "js_jsvalue_to_string_coerce");
    let last_operand = last_alloc(&ir);
    assert!(
        coerce > last_operand,
        "fork's `js_jsvalue_to_string_coerce` runs at line {coerce}, ABOVE the last operand's \
         allocation at line {last_operand}. Its result is a raw string pointer; carrying one \
         across a lowering is taxonomy (a) and no re-read can repair it.\n{ir}"
    );
}

// ---------------------------------------------------------------------------
// expr/proxy_reflect.rs
// ---------------------------------------------------------------------------

/// The canonical two-operand window: `target` live across `key`'s lowering.
#[test]
fn reflect_has_roots_its_target_across_the_key_operand() {
    let ir = compile_body(
        "reflect_has_window",
        vec![Stmt::Expr(Expr::ReflectHas {
            target: Box::new(allocating("t")),
            key: Box::new(allocating("k")),
        })],
    );
    assert_operand_survives_the_window(
        &ir,
        "js_reflect_has",
        0,
        "Reflect.has's target is live across the key's lowering",
    );
}

/// The zero-cost counterpart for the same family.
#[test]
fn reflect_own_keys_emits_no_rooting_traffic() {
    let ir = compile_body(
        "reflect_own_keys_cold",
        vec![Stmt::Expr(Expr::ReflectOwnKeys(Box::new(allocating("t"))))],
    );
    require_call_line(&ir, "js_reflect_own_keys");
    assert_eq!(
        temp_root_calls(&ir),
        0,
        "a single-operand Reflect helper has no window at all\n{ir}"
    );
}

/// Four operands, so `target` is live across three lowerings.
#[test]
fn reflect_set_roots_its_target_across_three_later_operands() {
    let ir = compile_body(
        "reflect_set_window",
        vec![Stmt::Expr(Expr::ReflectSet {
            target: Box::new(allocating("t")),
            key: Box::new(allocating("k")),
            value: Box::new(allocating("v")),
            receiver: Box::new(allocating("r")),
        })],
    );
    assert_operand_survives_the_window(
        &ir,
        "js_reflect_set",
        0,
        "Reflect.set's target is live across key, value and receiver",
    );
}

/// The `reflect-metadata` family shared one body and therefore one window.
#[test]
fn reflect_define_metadata_roots_its_key_across_the_later_operands() {
    let ir = compile_body(
        "define_metadata_window",
        vec![Stmt::Expr(Expr::ReflectDefineMetadata {
            key: Box::new(allocating("k")),
            value: Box::new(allocating("v")),
            target: Box::new(allocating("t")),
            property_key: Some(Box::new(allocating("p"))),
        })],
    );
    assert_operand_survives_the_window(
        &ir,
        "js_reflect_define_metadata",
        0,
        "Reflect.defineMetadata's key is live across value, target and propertyKey",
    );
}

/// `Proxy.apply` held the proxy in a bare register across `js_array_alloc` and
/// every argument's lowering, while the array itself was the #7154 accumulator.
#[test]
fn proxy_apply_rereads_its_receiver_below_the_argument_array() {
    let ir = compile_body(
        "proxy_apply_window",
        vec![Stmt::Expr(Expr::ProxyApply {
            proxy: Box::new(allocating("p")),
            args: vec![allocating("a"), allocating("b")],
        })],
    );
    let call = require_call_line(&ir, "js_proxy_apply");
    let proxy = call_operand(&ir, "js_proxy_apply", 0);
    let proxy_def = require_definition_line(&ir, &proxy);
    let last_push = ir
        .lines()
        .enumerate()
        .take(call)
        .filter(|(_, l)| l.contains("@js_array_push_f64"))
        .map(|(i, _)| i)
        .last()
        .unwrap_or_else(|| panic!("no argument push above the trap call in:\n{ir}"));
    assert!(
        proxy_def > last_push,
        "js_proxy_apply reads {proxy}, defined at line {proxy_def}, ABOVE the last argument \
         push at line {last_push}. Every push allocates, so the receiver must be re-read \
         below them.\n{ir}"
    );
}

/// The argument array is the accumulator: each push must READ it from the
/// rooted slot rather than from the previous push's return register, because
/// an unrelated collection during the NEXT argument's lowering moves it.
#[test]
fn proxy_apply_pushes_read_the_array_from_its_slot() {
    let ir = compile_body(
        "proxy_apply_accumulator",
        vec![Stmt::Expr(Expr::ProxyApply {
            proxy: Box::new(allocating("p")),
            args: vec![allocating("a"), allocating("b")],
        })],
    );
    require_call_line(&ir, "js_array_alloc");
    let pushes: Vec<usize> = ir
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("@js_array_push_f64") && !l.trim_start().starts_with("declare"))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(pushes.len(), 2, "expected one push per argument\n{ir}");

    // The array register the SECOND push reads must be defined below the
    // second argument's own allocation. Pre-fix it was the FIRST push's return
    // register, threaded across that lowering — the accumulator held the only
    // reference to argument `a` while argument `b` was evaluated.
    let second_line = ir.lines().nth(pushes[1]).expect("push line exists");
    let array_reg = second_line
        .rsplit_once('(')
        .expect("push has an argument list")
        .1
        .split(',')
        .next()
        .expect("push takes the array first")
        .trim()
        .rsplit(' ')
        .next()
        .expect("an operand is a type followed by a register")
        .to_string();
    let array_def = require_definition_line(&ir, &array_reg);
    let b_alloc = last_alloc_before(&ir, pushes[1]);
    assert!(
        array_def > b_alloc,
        "the second push reads {array_reg}, defined at line {array_def}, ABOVE the second \
         argument's allocation at line {b_alloc}. The accumulator held the ONLY reference to \
         everything pushed so far while that argument was lowered, and every push allocates — \
         it must be re-read from its rooted slot between pushes (#7154).\n{ir}"
    );
}

/// `process.env[computed] = value` stripped the COERCED key to a raw
/// `StringHeader*` above the value's lowering. Taxonomy (a) on a value with no
/// other root at all: `js_to_property_key` returns a fresh string.
#[test]
fn process_env_computed_key_strips_below_the_value() {
    let ir = compile_body(
        "process_env_computed",
        vec![Stmt::Expr(Expr::PutValueSet {
            target: Box::new(Expr::ProcessEnv),
            key: Box::new(allocating("k")),
            value: Box::new(allocating("v")),
            receiver: Box::new(Expr::ProcessEnv),
            strict: true,
        })],
    );
    require_call_line(&ir, "js_to_property_key");
    assert_operand_survives_the_window(
        &ir,
        "js_setenv",
        0,
        "process.env's coerced key is a fresh heap string with no other root",
    );
}

/// The literal-key branch needs no slot — its handle global is a registered
/// root — but the LOAD has to sit below the value's lowering, because
/// evacuation rewrites that global (#7114).
#[test]
fn process_env_literal_key_loads_below_the_value() {
    let ir = compile_body(
        "process_env_literal",
        vec![Stmt::Expr(Expr::PutValueSet {
            target: Box::new(Expr::ProcessEnv),
            key: Box::new(Expr::String("PATH".to_string())),
            value: Box::new(allocating("v")),
            receiver: Box::new(Expr::ProcessEnv),
            strict: true,
        })],
    );
    assert_operand_survives_the_window(
        &ir,
        "js_setenv",
        0,
        "the key handle global is one evacuation rewrites, so the load must follow the window",
    );
    assert_eq!(
        temp_root_calls(&ir),
        0,
        "a string-literal key takes `Reload`, not `Root`: no runtime call should be emitted\n{ir}"
    );
}

// ---------------------------------------------------------------------------
// expr/fs_await.rs
// ---------------------------------------------------------------------------

fn await_body(n: usize) -> Vec<Stmt> {
    (0..n)
        .map(|i| Stmt::Expr(Expr::Await(Box::new(allocating(&format!("p{i}"))))))
        .collect()
}

/// The await loop's temp root was pushed and never released, so N awaits in one
/// function held N slots to the end of the function — and in the FFI-fallback
/// lowering pushed N runtime entries per EXECUTION with no truncate.
///
/// Asserted as slot REUSE rather than by matching push/truncate text, because
/// reuse is the property the release buys in both lowerings: a released slot
/// returns to the pool, so three awaits cost exactly what one does.
#[test]
fn sequential_awaits_reuse_one_rooted_slot() {
    let one = compile_body("await_one", await_body(1));
    let three = compile_body("await_three", await_body(3));

    // Liveness: the arm under test must actually have run in both, and the
    // three-await module must really contain three of them. Without this a
    // pair of modules that both lowered nothing would compare equal.
    require_call_line(&one, "js_await_any_promise");
    assert_eq!(
        call_count(&one, "js_await_any_promise"),
        1,
        "expected exactly one await lowering\n{one}"
    );
    assert_eq!(
        call_count(&three, "js_await_any_promise"),
        3,
        "expected exactly three await lowerings\n{three}"
    );

    let one_slots = temp_root_slot_width(&one);
    let three_slots = temp_root_slot_width(&three);
    assert_eq!(
        one_slots, three_slots,
        "one await reserves {one_slots} rooted slot(s) and three reserve {three_slots}. The \
         await scope is not being released, so each await consumes a fresh slot instead of \
         reusing the pool — over-retention on every path, and in the FFI-fallback lowering an \
         unbounded runtime push per execution (#7462's shape).\n{three}"
    );
}

/// How many rooted slots the function reserves.
///
/// All three lowerings are covered, because which one runs is a build-time
/// property (`native_stack_roots_enabled`, `shadow_frame_requested`) and a
/// measure that only worked under one of them would silently stop measuring:
///
///  * **statepoint / RS4GC** (what this build uses): every pooled slot is an
///    entry alloca the retype pass rewrote to `alloca ptr addrspace(1)`;
///  * **shadow frame**: the width is `js_shadow_frame_enter`'s argument;
///  * **FFI fallback**: one `js_gc_temp_root_push` per acquisition.
fn temp_root_slot_width(ir: &str) -> usize {
    let gc_allocas = ir
        .lines()
        .filter(|l| l.contains("alloca ptr addrspace(1)"))
        .count();
    if gc_allocas > 0 {
        return gc_allocas;
    }
    if let Some(line) = ir
        .lines()
        .find(|l| l.contains("@js_shadow_frame_enter(") && !l.trim_start().starts_with("declare"))
    {
        let width = line
            .trim_end()
            .trim_end_matches(')')
            .rsplit(' ')
            .next()
            .and_then(|n| n.parse::<usize>().ok());
        if let Some(width) = width {
            return width;
        }
    }
    ir.lines()
        .filter(|l| !l.trim_start().starts_with("declare"))
        .filter(|l| l.contains("@js_gc_temp_root_push("))
        .count()
}

/// How many non-`declare` calls to `callee` the module emits.
fn call_count(ir: &str, callee: &str) -> usize {
    let needle = format!("@{callee}(");
    ir.lines()
        .filter(|l| !l.trim_start().starts_with("declare"))
        .filter(|l| l.contains(&needle))
        .count()
}
