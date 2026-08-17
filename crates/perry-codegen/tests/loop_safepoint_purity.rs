//! The loop back-edge GC poll (`js_gc_loop_safepoint`) and the purity proof
//! that removes it.
//!
//! `crate::loop_purity::loop_may_allocate` decides, per loop back edge,
//! whether a deferred minor collection could be waiting to be drained. Its
//! whitelist used to omit relational comparisons, arithmetic `Binary` and
//! `Update`, so `for (let i = 0; i < n; i++) { sum = sum + 1; }` failed the
//! test on its own condition, body AND update — three poll call sites per
//! iteration in a loop that allocates nothing.
//!
//! The widening reuses `expr_is_inert_primitive` (#6975): those operators run
//! ToPrimitive / ToNumeric, and a user-defined `valueOf` is arbitrary JS that
//! allocates. The unit tests in `loop_purity.rs` pin the walk with an injected
//! predicate; these compile real HIR so the REAL predicate — runtime-derived
//! stable types, whole-write numeric proofs, `shadow_slot_map`,
//! `module_globals` and all — is the thing under test.
//!
//! Every "no poll" assertion below is paired with a case that differs in one
//! operand and MUST keep its poll. That pairing is the point, and it was
//! verified by breaking the implementation five ways and checking that exactly
//! the intended tests went red:
//!
//! | sabotage                                            | red                                   |
//! |-----------------------------------------------------|---------------------------------------|
//! | `Add` loses its non-pointer operand condition        | `concatenating_two_string_literals…`  |
//! | `local_is_inert_primitive` drops the module-global guard | `a_module_global_accumulator…`    |
//! | the injected predicate answers `true` for everything | all four operand-proof cases          |
//! | the injected predicate answers `false` for everything (the pre-change behaviour) | `proven_numeric_counted_loop…` |
//! | `local_is_inert_primitive` drops the shadow-slot guard | *nothing* — see below               |
//!
//! That last row is honest rather than reassuring. The shadow-slot half of
//! `local_is_inert_primitive` is defensive redundancy: `collect_pointer_typed_locals`
//! reserves slots from a local's *declared/inferred* type, so any local whose
//! refined type is already `Number`/`Int32`/… has no slot either way, and no
//! fixture separates the two halves. It is kept because it is #6975's own
//! formulation and costs a hash lookup.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Module, ModuleInitKind, Stmt, UpdateOp};

fn entry_opts() -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: true,
        non_entry_module_prefixes: Vec::new(),
        nextjs_path_init_modules: Vec::new(),
        import_function_prefixes: std::collections::HashMap::new(),
        import_function_ffi_aliases: std::collections::HashMap::new(),
        import_function_origin_names: std::collections::HashMap::new(),
        import_function_v8_specifiers: std::collections::HashMap::new(),
        import_function_node_submodule: std::collections::HashMap::new(),
        namespace_node_submodules: std::collections::HashMap::new(),
        namespace_v8_specifiers: std::collections::HashMap::new(),
        namespace_member_prefixes: std::collections::HashMap::new(),
        namespace_member_origin_names: std::collections::HashMap::new(),
        emit_ir_only: true,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        namespace_imports: Vec::new(),
        namespace_member_nested: Vec::new(),
        imported_classes: Vec::new(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: std::collections::HashMap::new(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_synthetic_arguments: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        needs_stdlib: false,
        needs_ui: false,
        needs_geisterhand: false,
        geisterhand_port: 7676,
        enabled_features: Vec::new(),
        native_module_init_names: Vec::new(),
        js_module_specifiers: Vec::new(),
        bundled_extensions: Vec::new(),
        native_library_functions: Vec::new(),
        i18n_table: None,
        fast_math: false,
        fp_contract_mode: perry_codegen::FpContractMode::Off,
        app_metadata: AppMetadata::default(),
        namespace_entries: Vec::new(),
        dynamic_import_path_to_prefix: std::collections::HashMap::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}
fn module_with_init(name: &str, init: Vec<Stmt>) -> Module {
    Module {
        name: name.to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: Vec::new(),
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        init,
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        local_source_spans: std::collections::HashMap::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

/// The back-edge poll is gated behind `PERRY_GC_MOVING_LOOP_POLLS`, which
/// defaults OFF. Every assertion in this file is about WHICH loops keep the
/// poll, so with the knob unset the emitter returns before the purity analysis
/// runs and six of these tests fail for a reason that has nothing to do with
/// what they test.
///
/// That is how they went red: the knob arrived with the moving-GC work and
/// nothing here turned it on, while integration suites under `crates/*/tests/`
/// do not run per-PR, so nobody saw it. `moving_safepoint_polls_enabled`
/// caches in a `OnceLock`, so this must run before the first codegen call —
/// every helper that produces IR goes through here.
fn enable_back_edge_polls() {
    // Safe on edition 2021, and every test in this binary wants the same value.
    std::env::set_var("PERRY_GC_MOVING_LOOP_POLLS", "1");
}

fn ir_for(name: &str, init: Vec<Stmt>) -> String {
    enable_back_edge_polls();
    String::from_utf8(compile_module(&module_with_init(name, init), entry_opts()).unwrap())
        .expect("LLVM IR should be UTF-8")
}

/// Same, but `exported` names are exported module variables — which is what
/// promotes a module-level `let` to a `@perry_global_*` slot.
fn ir_for_with_exported_vars(name: &str, init: Vec<Stmt>, exported: &[&str]) -> String {
    enable_back_edge_polls();
    let mut m = module_with_init(name, init);
    m.exported_objects = exported.iter().map(|s| s.to_string()).collect();
    String::from_utf8(compile_module(&m, entry_opts()).unwrap()).expect("LLVM IR should be UTF-8")
}

/// A CALL to the poll. The `declare void @js_gc_loop_safepoint()` line is
/// emitted unconditionally, so only a call site counts.
const POLL: &str = "call void @js_gc_loop_safepoint()";

const N: u32 = 1;
const SUM: u32 = 2;
const I: u32 = 3;

fn let_stmt(id: u32, name: &str, ty: Type, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty,
        mutable: true,
        init: Some(init),
    }
}

/// `for (let i = 0; i < n; i++) { <body> }`, with `n` and `sum` declared at
/// module scope from the caller's declarations.
///
/// Note that the DECLARED type is not what decides inertness — the pointer
/// scan tracks the values a local actually receives, so `let n: any = 1000`
/// is still proven numeric, and correctly so. To make a local coercible it has
/// to actually be handed a heap value, which is what `coercible()` does.
fn counted_loop(n: Stmt, sum: Stmt, body: Vec<Stmt>) -> Vec<Stmt> {
    let mut stmts = vec![n, sum];
    stmts.extend(counted_loop_only(body));
    stmts
}

/// Just the `for` statement, for fixtures that declare `n` / `sum` themselves.
fn counted_loop_only(body: Vec<Stmt>) -> Vec<Stmt> {
    vec![Stmt::For {
        init: Some(Box::new(let_stmt(I, "i", Type::Number, Expr::Number(0.0)))),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(I)),
            right: Box::new(Expr::LocalGet(N)),
        }),
        update: Some(Expr::Update {
            id: I,
            op: UpdateOp::Increment,
            prefix: false,
        }),
        body,
    }]
}

/// `sum = sum + <rhs>`.
/// A local that provably holds a heap value: `any`-typed and initialized with
/// an object. Whatever that object's `valueOf` does is arbitrary JS, so every
/// coercion of this local is a potential allocation — and the shadow-slot scan
/// reserves it a slot, which is the fact `expr_is_inert_primitive` reads.
fn coercible(id: u32, name: &str) -> Stmt {
    let_stmt(id, name, Type::Any, Expr::Object(Vec::new()))
}

/// A local proven to hold a number.
fn numeric(id: u32, name: &str, v: f64) -> Stmt {
    let_stmt(id, name, Type::Number, Expr::Number(v))
}

fn accumulate(rhs: Expr) -> Vec<Stmt> {
    vec![Stmt::Expr(Expr::LocalSet(
        SUM,
        Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(SUM)),
            right: Box::new(rhs),
        }),
    ))]
}

// ------------------------------------------------------------ the win ------

/// The headline case. Condition, body and update are all proven-numeric, so
/// none of the three back edges may emit a poll.
#[test]
fn proven_numeric_counted_loop_emits_no_back_edge_poll() {
    let ir = ir_for(
        "loop_poll_numeric.ts",
        counted_loop(
            numeric(N, "n", 1000.0),
            numeric(SUM, "sum", 0.0),
            accumulate(Expr::Number(1.0)),
        ),
    );
    assert!(
        !ir.contains(POLL),
        "a loop that allocates nothing must emit no back-edge poll — \
         `i < n`, `sum + 1` and `i++` are all over proven-numeric locals:\n{ir}"
    );
}

// ---------------------------------------------------- the safe direction ---

/// Sabotage #1 — the loop BOUND becomes an object. Nothing else changes: same
/// condition node, same body, same update. `i < n` now runs ToPrimitive on a
/// value that can define `valueOf`, so the poll has to survive.
///
/// This is the pairing that gives the test above its teeth: a predicate that
/// waved everything through would pass that one and fail this one.
#[test]
fn a_coercible_bound_keeps_the_back_edge_poll() {
    let ir = ir_for(
        "loop_poll_any_bound.ts",
        counted_loop(
            coercible(N, "n"),
            numeric(SUM, "sum", 0.0),
            accumulate(Expr::Number(1.0)),
        ),
    );
    assert!(
        ir.contains(POLL),
        "`i < n` over an object-valued bound can run a user `valueOf`, which \
         allocates — the poll must survive:\n{ir}"
    );
}

/// Sabotage #2 — the ACCUMULATOR becomes an object, so `sum + 1` is the
/// coercing operator instead of the comparison.
#[test]
fn a_coercible_accumulator_keeps_the_back_edge_poll() {
    let ir = ir_for(
        "loop_poll_any_acc.ts",
        counted_loop(
            numeric(N, "n", 1000.0),
            coercible(SUM, "sum"),
            accumulate(Expr::Number(1.0)),
        ),
    );
    assert!(
        ir.contains(POLL),
        "`sum + 1` over an object-valued accumulator can run a user `valueOf`:\n{ir}"
    );
}

/// Sabotage #3 — `+` over two string LITERALS. This is the case that isolates
/// the `Add` rule and nothing else: a string literal is inert (ToPrimitive on
/// it is the identity, no user code), so "both operands inert" is satisfied —
/// and the concatenation still allocates a fresh string every iteration.
///
/// `Add` therefore carries an extra condition that neither operand may BE a
/// heap reference. Delete it — leave `Add` merely "inert operands" — and this
/// test goes red while every other test in the file stays green.
#[test]
fn concatenating_two_string_literals_keeps_the_back_edge_poll() {
    let ir = ir_for(
        "loop_poll_concat.ts",
        counted_loop(
            numeric(N, "n", 1000.0),
            let_stmt(SUM, "sum", Type::String, Expr::String(String::new())),
            vec![Stmt::Expr(Expr::LocalSet(
                SUM,
                Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::String("a".to_string())),
                    right: Box::new(Expr::String("b".to_string())),
                }),
            ))],
        ),
    );
    assert!(
        ir.contains(POLL),
        "`\"a\" + \"b\"` allocates a fresh string every iteration, even though \
         both operands are inert:\n{ir}"
    );
}

/// Sabotage #4 — an outright allocation in the body. The catch-all has always
/// covered this; it must keep covering it.
#[test]
fn an_object_literal_in_the_body_keeps_the_back_edge_poll() {
    let ir = ir_for(
        "loop_poll_object.ts",
        counted_loop(
            numeric(N, "n", 1000.0),
            let_stmt(SUM, "sum", Type::Any, Expr::Undefined),
            vec![Stmt::Expr(Expr::LocalSet(
                SUM,
                Box::new(Expr::Object(Vec::new())),
            ))],
        ),
    );
    assert!(
        ir.contains(POLL),
        "an object literal per iteration is exactly what the poll exists \
         for:\n{ir}"
    );
}

/// Sabotage #5 — a CALL in the body. A call can reach anything, including a
/// collection, so no amount of numeric proof around it may drop the poll.
#[test]
fn a_call_in_the_body_keeps_the_back_edge_poll() {
    let ir = ir_for(
        "loop_poll_call.ts",
        counted_loop(
            numeric(N, "n", 1000.0),
            numeric(SUM, "sum", 0.0),
            vec![Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::LocalGet(SUM)),
                args: vec![Expr::Number(1.0)],
                type_args: Vec::new(),
                byte_offset: 0,
            })],
        ),
    );
    assert!(
        ir.contains(POLL),
        "a call in the body can reach a collection point:\n{ir}"
    );
}

/// Sabotage #6 — the accumulator is a module-level EXPORTED binding, so it
/// lives in a `@perry_global_*` slot rather than an alloca.
///
/// Everything the inertness proof reads — the refined type, the shadow-slot
/// map — is computed from THIS function's body alone, and a module global can
/// be assigned an object by a different function or a different module that
/// this scan never sees. `local_is_inert_primitive` therefore refuses module
/// globals outright, and this pins that: the loop is textually identical to
/// the proven-numeric one, and must still keep its poll.
#[test]
fn a_module_global_accumulator_keeps_the_back_edge_poll() {
    let init = counted_loop(
        numeric(N, "n", 1000.0),
        numeric(SUM, "sum", 0.0),
        accumulate(Expr::Number(1.0)),
    );
    let ir = ir_for_with_exported_vars("loop_poll_module_global.ts", init, &["sum"]);
    assert!(
        ir.contains("@perry_global_"),
        "the fixture only means something if `sum` really became a module \
         global:\n{ir}"
    );
    assert!(
        ir.contains(POLL),
        "a module global's type and pointer-ness are only known per function, \
         so it is never inert and the poll must survive:\n{ir}"
    );
}

// ------------------------------------------------- the poll's own price ----

/// A poll that survives the purity proof must still be GUARDED: an inline load
/// of the runtime's arming word, and the call only on the arm where it is
/// non-zero.
///
/// This is the other half of "where the poll goes". `loop_may_allocate` decides
/// WHICH loops carry a poll; this decides what carrying one COSTS. A bare
/// `call void @js_gc_loop_safepoint()` at every allocating back-edge is not a
/// small constant — its no-work path went through two `OnceLock` acquire loads,
/// an unconditional atomic increment and a thread-local read, and on Darwin a
/// thread-local read is a call to `_tlv_get_addr`. At 20 M back-edges per
/// `bench/churn_alloc.ts` run that measured 0.36 s -> 0.42 s, `push_cls`
/// 0.34 -> 0.40 and `push_num` 0.13 -> 0.17 when #7721 turned the polls on.
///
/// The guard has to be checked here rather than by reading the emitter, because
/// nothing else can fail when it goes away: dropping the `cond_br` leaves a
/// program that is correct, passes every other test in this file, and is 15-30 %
/// slower on exactly the workloads that carry polls.
#[test]
fn a_surviving_poll_is_guarded_by_the_arming_word() {
    let ir = ir_for(
        "loop_poll_guarded.ts",
        counted_loop(
            coercible(N, "n"),
            numeric(SUM, "sum", 0.0),
            accumulate(Expr::Number(1.0)),
        ),
    );
    assert!(
        ir.contains(POLL),
        "fixture premise: this loop must still carry a poll:\n{ir}"
    );
    assert!(
        ir.contains("@PERRY_GC_POLL_ARMED = external global i32"),
        "the arming word must be declared — the runtime defines it in \
         gc/poll_arm.rs:\n{ir}"
    );
    assert!(
        ir.contains("load volatile i32, ptr @PERRY_GC_POLL_ARMED"),
        "the guard must be a VOLATILE load: the runtime writes this word from \
         calls LLVM cannot see through, and a hoisted or CSE'd load reads a \
         stale zero and silently stops draining deferred collections:\n{ir}"
    );
    // One guard per poll, not one guard for the function.
    assert_eq!(
        ir.matches("load volatile i32, ptr @PERRY_GC_POLL_ARMED")
            .count(),
        ir.matches(POLL).count(),
        "every emitted poll needs its own guard:\n{ir}"
    );
    let guarded = ir
        .split("load volatile i32, ptr @PERRY_GC_POLL_ARMED")
        .skip(1)
        .all(|after| {
            // The property is about the CFG, not about text order: the load's
            // own block must end in a conditional branch, and the call must
            // not be in that block — it belongs to a successor.
            //
            // Asking instead for "the first `br i1` appears before the first
            // POLL anywhere after the load" silently depended on the poll
            // blocks being emitted next to their guards. They are not: the
            // guards sit inline in `for.cond` / `for.body` / `for.update`,
            // while every `gcpoll.N` block is emitted together after the loop.
            // Under that layout the first two segments contain no call at all,
            // so the old check read `None` and failed a correctly-guarded
            // poll. Bounding the search at the block edge asserts the real
            // thing and does not care where the successor is printed.
            let block_tail = after.split("\n\n").next().unwrap_or(after);
            block_tail.contains("br i1 ") && !block_tail.contains(POLL)
        });
    assert!(
        guarded,
        "the call must sit behind the branch, not after the load in the same \
         block — otherwise the load is decoration:\n{ir}"
    );
}
