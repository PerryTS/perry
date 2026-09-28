//! Ground truth for the S4 liveness count: every fixture runs the shipped
//! split pipeline (prepare, count, rewrite) and asserts the prediction equals
//! the `gc.relocate`s RS4GC actually emitted, and a pinned number so a change
//! that moves both in step is still visible.

use super::super::{default_cpu_for_triple, global_init, parse_ir_text};
use super::*;
use inkwell::context::Context;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{CodeModel, RelocMode, Target, TargetMachine, TargetTriple};
use inkwell::values::AsValueRef;
use inkwell::OptimizationLevel;

const TRIPLE: &str = "arm64-apple-darwin";

fn target_machine() -> TargetMachine {
    global_init(&[]);
    let triple = TargetTriple::create(TRIPLE);
    Target::from_triple(&triple)
        .expect("aarch64 target")
        .create_target_machine(
            &triple,
            default_cpu_for_triple(TRIPLE),
            "",
            OptimizationLevel::None,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .expect("target machine")
}

fn run(module: &inkwell::module::Module<'_>, tm: &TargetMachine, passes: &str) {
    module
        .run_passes(passes, tm, PassBuilderOptions::create())
        .unwrap_or_else(|e| panic!("`{passes}` failed: {e}"));
}

/// The prediction for `name` on RS4GC's real input, and the `gc.relocate`s
/// RS4GC then really emitted for it.
fn census(ir: &str, name: &str) -> (FunctionLiveness, u64) {
    let tm = target_machine();
    let context = Context::create();
    let module = parse_ir_text(&context, ir, "gc_liveness_fixture").expect("fixture parses");
    module.set_triple(&TargetTriple::create(TRIPLE));
    module.set_data_layout(&tm.get_target_data().get_data_layout());
    module.verify().expect("fixture verifies");
    run(&module, &tm, crate::linker::STATEPOINT_PREPARE_PASSES);
    let f = module.get_function(name).expect("function survives");
    let predicted = analyze_function(f.as_value_ref());
    run(&module, &tm, crate::linker::STATEPOINT_REWRITE_ONLY_PASSES);
    module.verify().expect("rewritten module verifies");
    let f = module.get_function(name).expect("function survives RS4GC");
    (predicted, count_relocates(f.as_value_ref()))
}

/// Assert exactness against RS4GC and pin the count and the live sets.
fn assert_census(ir: &str, name: &str, relocations: u64, live: &[u32]) -> FunctionLiveness {
    let (l, actual) = census(ir, name);
    assert_eq!(
        l.relocations, actual,
        "`{name}`: predicted {} relocations, RS4GC emitted {actual}\n{l:#?}",
        l.relocations
    );
    assert!(l.is_exact(), "`{name}` has no derived pointers: {l:#?}");
    assert_eq!(l.relocations, relocations, "`{name}` pinned count: {l:#?}");
    let got: Vec<u32> = l.safepoints.iter().map(|s| s.live).collect();
    assert_eq!(got, live, "`{name}` per-safepoint live counts: {l:#?}");
    l
}

const DECLS: &str = r#"
declare void @may_collect()
declare ptr addrspace(1) @alloc()
declare void @use(ptr addrspace(1))
declare void @use2(ptr addrspace(1), ptr addrspace(1))
declare i32 @__gxx_personality_v0(...)
"#;

fn ir(body: &str) -> String {
    format!("{DECLS}\n{body}")
}

#[test]
fn straight_line_counts_values_live_after_each_call() {
    // A call's own result is not live across it; its GC-pointer arguments
    // are, even at their last use.
    let l = assert_census(
        &ir(r#"
define void @f() gc "statepoint-example" {
entry:
  %a = call ptr addrspace(1) @alloc()
  %b = call ptr addrspace(1) @alloc()
  call void @may_collect()
  call void @use(ptr addrspace(1) %a)
  call void @use(ptr addrspace(1) %b)
  ret void
}
"#),
        "f",
        6,
        &[0, 1, 2, 2, 1],
    );
    assert_eq!(
        (l.statepoints(), l.invoke_statepoints(), l.max_live()),
        (5, 0, 2)
    );
}

#[test]
fn root_allocas_are_counted_as_the_ssa_values_mem2reg_makes() {
    // The pre-mem2reg shape codegen emits: roots live in allocas.
    assert_census(
        &ir(r#"
define void @slots() gc "statepoint-example" {
entry:
  %slot = alloca ptr addrspace(1)
  %dead = alloca ptr addrspace(1)
  %a = call ptr addrspace(1) @alloc()
  store ptr addrspace(1) %a, ptr %slot
  store ptr addrspace(1) null, ptr %dead
  call void @may_collect()
  %r = load ptr addrspace(1), ptr %slot
  %d = load ptr addrspace(1), ptr %dead
  call void @use2(ptr addrspace(1) %r, ptr addrspace(1) %d)
  ret void
}
"#),
        "slots",
        // `%a` across may_collect and into `use2`; the null slot is a
        // constant after mem2reg.
        2,
        &[0, 1, 1],
    );
}

#[test]
fn loops_and_phis_follow_the_cfg() {
    assert_census(
        &ir(r#"
define void @loop(i64 %n) gc "statepoint-example" {
entry:
  %keep = call ptr addrspace(1) @alloc()
  br label %head
head:
  %i = phi i64 [ 0, %entry ], [ %i1, %body ]
  %cur = phi ptr addrspace(1) [ %keep, %entry ], [ %next, %body ]
  %c = icmp slt i64 %i, %n
  br i1 %c, label %body, label %exit
body:
  %next = call ptr addrspace(1) @alloc()
  call void @may_collect()
  %i1 = add i64 %i, 1
  br label %head
exit:
  call void @use(ptr addrspace(1) %cur)
  call void @use(ptr addrspace(1) %keep)
  ret void
}
"#),
        "loop",
        // alloc in the loop: {keep}; may_collect: {keep, next}; the uses in
        // exit: {cur, keep}, then {keep}. `%cur` is redefined at the header,
        // so it is not live across the body's calls.
        6,
        &[0, 1, 2, 2, 1],
    );
}

const INVOKE: &str = r#"
define void @inv() gc "statepoint-example" personality ptr @__gxx_personality_v0 {
entry:
  %a = call ptr addrspace(1) @alloc()
  %b = call ptr addrspace(1) @alloc()
  invoke void @may_collect() to label %ok unwind label %lp
ok:
  call void @use(ptr addrspace(1) %a)
  ret void
lp:
  %l = landingpad token cleanup
  call void @use(ptr addrspace(1) %b)
  ret void
}
"#;

#[test]
fn an_invoke_is_relocated_on_the_normal_and_the_unwind_edge() {
    // `%a` is live into the normal destination, `%b` into the landing pad
    // (Perry's retyped `landingpad token`); RS4GC relocates the union on BOTH
    // edges: 2 values x 2 edges.
    let l = assert_census(&ir(INVOKE), "inv", 1 + 4 + 1 + 1, &[0, 1, 2, 1, 1]);
    assert_eq!(l.invoke_statepoints(), 1);
    // Sabotage witness: a count that ignored the unwind edge (sum of live
    // counts) would be 5, not RS4GC's 7 — this fixture tells them apart.
    let naive: u64 = l.safepoints.iter().map(|s| u64::from(s.live)).sum();
    assert_eq!(naive, 5);
    assert_ne!(naive, l.relocations);
}

#[test]
fn an_invoke_of_a_nounwind_callee_becomes_a_call() {
    // markAliveBlocks turns it into a call before RS4GC: one edge, and the
    // landing pad becomes unreachable.
    assert_census(
        &ir(r#"
declare void @nothrow() nounwind
define void @nu() gc "statepoint-example" personality ptr @__gxx_personality_v0 {
entry:
  %a = call ptr addrspace(1) @alloc()
  %b = call ptr addrspace(1) @alloc()
  invoke void @nothrow() to label %ok unwind label %lp
ok:
  call void @use(ptr addrspace(1) %a)
  ret void
lp:
  %l = landingpad token cleanup
  call void @use(ptr addrspace(1) %b)
  ret void
}
"#),
        "nu",
        1 + 1 + 1,
        &[0, 1, 1, 1],
    );
}

#[test]
fn leaf_calls_are_not_safepoints() {
    // Call-site attribute, callee attribute, intrinsic, inline asm, and a C
    // library function TargetLibraryInfo knows: none is a statepoint, so `%a`
    // is live across exactly one call.
    let l = assert_census(
        &ir(r#"
declare void @leaf_decl() "gc-leaf-function"
declare void @plain()
declare i64 @strlen(ptr)
declare void @llvm.donothing()
define i64 @leafy(ptr %s) gc "statepoint-example" {
entry:
  %a = call ptr addrspace(1) @alloc()
  call void @plain() "gc-leaf-function"
  call void @leaf_decl()
  %n = call i64 @strlen(ptr %s)
  call void @llvm.donothing()
  call void asm sideeffect "", ""() "gc-leaf-function"
  call void @may_collect()
  call void @use(ptr addrspace(1) %a)
  ret i64 %n
}
"#),
        "leafy",
        2,
        &[0, 1, 1],
    );
    assert_eq!(l.statepoints(), 3);
}

#[test]
fn a_value_reloaded_from_its_global_after_a_safepoint_is_not_live_across_it() {
    // S3's rematerialized form (after mem2reg/sccp fold its select): each
    // read is a fresh load of the global below the safepoint. The control
    // loads once and holds the value across both calls.
    let remat = assert_census(
        &ir(r#"
@G = global ptr addrspace(1) null
define void @remat() gc "statepoint-example" {
entry:
  call void @may_collect()
  %v1 = load ptr addrspace(1), ptr @G
  call void @use(ptr addrspace(1) %v1)
  call void @may_collect()
  %v2 = load ptr addrspace(1), ptr @G
  call void @use(ptr addrspace(1) %v2)
  ret void
}
"#),
        "remat",
        // Only each load's own use: nothing is live across `may_collect`.
        2,
        &[0, 1, 0, 1],
    );
    assert_eq!(remat.statepoints(), 4);
    assert_census(
        &ir(r#"
@G = global ptr addrspace(1) null
define void @held() gc "statepoint-example" {
entry:
  %v = load ptr addrspace(1), ptr @G
  call void @may_collect()
  call void @use(ptr addrspace(1) %v)
  call void @may_collect()
  call void @use(ptr addrspace(1) %v)
  ret void
}
"#),
        "held",
        4,
        &[1, 1, 1, 1],
    );
}

#[test]
fn code_after_a_noreturn_call_is_dead() {
    assert_census(
        &ir(r#"
declare void @die() noreturn
define void @nr(i1 %c) gc "statepoint-example" {
entry:
  %a = call ptr addrspace(1) @alloc()
  br i1 %c, label %bad, label %good
bad:
  call void @die()
  call void @use(ptr addrspace(1) %a)
  ret void
good:
  call void @may_collect()
  ret void
}
"#),
        "nr",
        // `die` is a safepoint, but nothing is live across it: the use after
        // it is deleted as unreachable.
        0,
        &[0, 0, 0],
    );
}

#[test]
fn a_single_use_compare_is_moved_to_its_branch() {
    // RS4GC sinks the icmp below the safepoint, so `%a` is live across it
    // even though its last textual use is above.
    assert_census(
        &ir(r#"
define void @cmp() gc "statepoint-example" {
entry:
  %a = call ptr addrspace(1) @alloc()
  %c = icmp eq ptr addrspace(1) %a, null
  call void @may_collect()
  br i1 %c, label %x, label %y
x:
  ret void
y:
  ret void
}
"#),
        "cmp",
        1,
        &[0, 1],
    );
}

#[test]
fn a_single_entry_phi_is_folded_into_its_input() {
    // Without the fold, `%a` and `%p` would count as two live values.
    assert_census(
        &ir(r#"
define void @one(i1 %c) gc "statepoint-example" {
entry:
  %a = call ptr addrspace(1) @alloc()
  br label %next
next:
  %p = phi ptr addrspace(1) [ %a, %entry ]
  call void @may_collect()
  call void @use2(ptr addrspace(1) %a, ptr addrspace(1) %p)
  ret void
}
"#),
        "one",
        2,
        &[0, 1, 1],
    );
}

#[test]
fn phi_bases_follow_rs4gc_find_base_pointer() {
    // Perry's phis often merge a NaN-box tag constant with a heap pointer.
    // RS4GC gives such a phi a fresh `.base` phi, live wherever it is: two
    // relocations per crossing. A phi with a `null` input is its own base,
    // and a phi of constants only has a constant base and is not relocated.
    let l = assert_census(
        &ir(r#"
declare void @sink(i64, i64, i64)
define void @tagphi(i1 %c) gc "statepoint-example" {
entry:
  %a = call ptr addrspace(1) @alloc()
  br i1 %c, label %t, label %j
t:
  br label %j
j:
  %p = phi ptr addrspace(1) [ inttoptr (i64 9222246136947933185 to ptr addrspace(1)), %t ], [ %a, %entry ]
  %n = phi ptr addrspace(1) [ null, %t ], [ %a, %entry ]
  %k = phi ptr addrspace(1) [ inttoptr (i64 9222246136947933185 to ptr addrspace(1)), %t ], [ inttoptr (i64 9222246136947933186 to ptr addrspace(1)), %entry ]
  call void @may_collect()
  %x = ptrtoint ptr addrspace(1) %p to i64
  %y = ptrtoint ptr addrspace(1) %n to i64
  %z = ptrtoint ptr addrspace(1) %k to i64
  call void @sink(i64 %x, i64 %y, i64 %z) "gc-leaf-function"
  ret void
}
"#),
        "tagphi",
        3,
        &[0, 3],
    );
    assert_eq!(l.fresh_bases, 1, "{l:#?}");
}

#[test]
fn derived_pointers_make_the_count_a_bound() {
    // A gep of a GC pointer: RS4GC relocates the derived value and adds its
    // base. The count is flagged as a bound, and the bound holds.
    let (l, actual) = census(
        &ir(r#"
define void @der() gc "statepoint-example" {
entry:
  %a = call ptr addrspace(1) @alloc()
  %d = getelementptr i8, ptr addrspace(1) %a, i64 8
  call void @may_collect()
  call void @use(ptr addrspace(1) %d)
  ret void
}
"#),
        "der",
    );
    assert!(!l.is_exact(), "{l:#?}");
    assert!(
        l.relocations <= actual && actual <= l.relocation_bound(),
        "{l:#?} vs {actual}"
    );
}

/// The in-process backend runs the pipeline in two halves around the count.
/// They must still spell exactly the shipped pipeline, and produce the same
/// module as the one-shot run.
#[test]
fn statepoint_pipeline_split_is_the_shipped_pipeline() {
    assert_eq!(
        format!(
            "{},{}",
            crate::linker::STATEPOINT_PREPARE_PASSES,
            crate::linker::STATEPOINT_REWRITE_ONLY_PASSES
        ),
        crate::linker::STATEPOINT_REWRITE_PASSES
    );
    let fixture = ir(INVOKE);
    let tm = target_machine();
    let rewrite = |split: bool| {
        let context = Context::create();
        let module = parse_ir_text(&context, &fixture, "split").expect("parses");
        module.set_triple(&TargetTriple::create(TRIPLE));
        module.set_data_layout(&tm.get_target_data().get_data_layout());
        if split {
            run(&module, &tm, crate::linker::STATEPOINT_PREPARE_PASSES);
            run(&module, &tm, crate::linker::STATEPOINT_REWRITE_ONLY_PASSES);
        } else {
            run(&module, &tm, crate::linker::STATEPOINT_REWRITE_PASSES);
        }
        module.print_to_string().to_string()
    };
    assert_eq!(rewrite(true), rewrite(false));
}

/// The spill decision flips exactly at the budget, on the exact count.
#[test]
fn the_spill_decision_flips_at_the_threshold() {
    let tm = target_machine();
    let context = Context::create();
    let module = parse_ir_text(&context, &ir(INVOKE), "flip").expect("parses");
    module.set_triple(&TargetTriple::create(TRIPLE));
    module.set_data_layout(&tm.get_target_data().get_data_layout());
    let rewritten = super::super::rs4gc_functions(&module);
    assert!(rewritten.contains("inv"));
    run(&module, &tm, crate::linker::STATEPOINT_PREPARE_PASSES);
    let liveness = analyze_module(&module, &rewritten);
    assert_eq!(liveness.len(), 1);
    assert_eq!(liveness[0].1.relocations, 7);

    let pre = super::super::pre_rewrite_sizes(&module);
    super::super::enforce_rs4gc_preflight_budget(&liveness, 7, &pre, None)
        .expect("7 relocations are within a budget of 7");
    super::super::enforce_rs4gc_preflight_budget(&liveness, 0, &pre, None)
        .expect("a budget of 0 disables spilling");
    let err = super::super::enforce_rs4gc_preflight_budget(&liveness, 6, &pre, None)
        .expect_err("7 relocations exceed a budget of 6");
    let retry = super::super::rs4gc_budget_retry(&err).expect("the request stays typed");
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].name, "inv");
    assert_eq!(retry[0].cap, 6);
    assert_eq!(
        retry[0].cause,
        super::super::Rs4gcBudgetCause::PreRewrite {
            statepoints: 5,
            invoke_statepoints: 1,
            max_live: 2,
            relocations: 7,
        }
    );
    let msg = format!("{err:#}");
    for needle in [
        "before rewrite-statepoints-for-gc",
        "`inv`",
        "5 statepoints",
        "1 of them invokes",
        "up to 2 GC values",
        "would emit 7 relocations",
        "budget 6",
        "PERRY_ROOT_SPILL_RELOCATIONS",
        "re-lower",
    ] {
        assert!(
            msg.contains(needle),
            "message must carry {needle:?}:\n{msg}"
        );
    }
    // A function outside RS4GC (a shadow-frame function) is never counted.
    assert!(analyze_module(&module, &std::collections::HashSet::new()).is_empty());
}

/// #11624 follow-up: a function can sit comfortably under the relocation cap
/// and still be predicted to cross the fast-emit machine-pipeline budget —
/// the actual claude-code shape (`__25747`: 0.4 M relocations, under the
/// 1.5 Mi cap, but its rewritten body crossed the 600 k x86-64 fast-emit
/// ceiling and fell back to LLVM's O0 pipeline). The relocation cap alone
/// must not catch this; the fast-emit prediction must.
#[test]
fn a_function_under_the_relocation_cap_but_over_the_fast_emit_budget_spills() {
    let tm = target_machine();
    let context = Context::create();
    let module = parse_ir_text(&context, &ir(INVOKE), "fast-emit-cliff").expect("parses");
    module.set_triple(&TargetTriple::create(TRIPLE));
    module.set_data_layout(&tm.get_target_data().get_data_layout());
    let rewritten = super::super::rs4gc_functions(&module);
    run(&module, &tm, crate::linker::STATEPOINT_PREPARE_PASSES);
    let liveness = analyze_module(&module, &rewritten);
    assert_eq!(liveness.len(), 1);
    assert_eq!(liveness[0].1.relocations, 7);
    let pre = super::super::pre_rewrite_sizes(&module);

    // Comfortably under the default relocation cap, and with no fast-emit
    // budget in play (`None`), the fixture must not spill.
    super::super::enforce_rs4gc_preflight_budget(
        &liveness,
        super::super::DEFAULT_ROOT_SPILL_RELOCATIONS,
        &pre,
        None,
    )
    .expect("7 relocations must not trip the default relocation cap");

    // A fast-emit budget far below the fixture's predicted post-rewrite size
    // (pre-rewrite instructions plus 2x its 7 relocations) must spill it,
    // even though the relocation cap is untouched.
    let err = super::super::enforce_rs4gc_preflight_budget(
        &liveness,
        super::super::DEFAULT_ROOT_SPILL_RELOCATIONS,
        &pre,
        Some(5),
    )
    .expect_err("a predicted post-rewrite size over a 5-instruction fast-emit budget must spill");
    let retry = super::super::rs4gc_budget_retry(&err).expect("the request stays typed");
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].name, "inv");
    assert_eq!(retry[0].cap, 5);
    match &retry[0].cause {
        super::super::Rs4gcBudgetCause::PredictedFastEmit {
            relocations,
            predicted_instructions,
        } => {
            assert_eq!(*relocations, 7);
            assert!(
                *predicted_instructions > 5,
                "predicted {predicted_instructions} must exceed the 5-instruction budget"
            );
        }
        other => panic!("expected PredictedFastEmit, got {other:?}"),
    }
    let msg = format!("{err:#}");
    for needle in [
        "before rewrite-statepoints-for-gc",
        "`inv`",
        "would emit 7 relocations",
        "under the relocation cap",
        "fast-emit",
        "budget 5",
        "PERRY_LL_FAST_EMIT_MAX_INSTRS",
    ] {
        assert!(
            msg.contains(needle),
            "message must carry {needle:?}:\n{msg}"
        );
    }

    // 0 disables spilling entirely, including the fast-emit prediction.
    super::super::enforce_rs4gc_preflight_budget(&liveness, 0, &pre, Some(5))
        .expect("a budget of 0 disables spilling entirely, even under a tiny fast-emit cap");
}

/// The relocation budget is the post-RS4GC instruction budget: every
/// relocation is one instruction of the rewritten body, so a function over it
/// would be spilled after RS4GC anyway. Change one only with the other.
#[test]
fn the_relocation_budget_is_the_post_rewrite_instruction_budget() {
    assert_eq!(
        super::super::DEFAULT_ROOT_SPILL_RELOCATIONS,
        super::super::DEFAULT_RS4GC_MAX_INSTRS as u64
    );
}
