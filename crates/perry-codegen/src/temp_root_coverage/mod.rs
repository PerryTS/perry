//! The #6951 temp-root emission contract, in the per-PR `cargo-test` gate
//! (#6988), asserted in a form both lowerings satisfy (#7503).
//!
//! # Why it moved here
//!
//! This contract used to live only in `tests/temp_root_argument_temporaries.rs`.
//! Integration suites under `crates/*/tests/*.rs` run nightly/at-tag, not
//! per-PR (#5960), so the assertions pinning the rooting emission contract were
//! CI-invisible on exactly the PRs most likely to break them — and the failure
//! mode is silent: a lowering that goes back to threading a value through an
//! SSA register produces correct output under the default configuration,
//! because the conservative native-stack scan pins it by accident, and diverges
//! only under `PERRY_CONSERVATIVE_STACK_SCAN=off` or an evacuating minor.
//!
//! #6983 declined the move because the assertions needed ~90 lines of
//! `entry_opts()` / `module_with_init()` harness and doing that for three tests
//! was not worth it. Doing it once for the family is, and #7653's
//! `native_root_coverage` had meanwhile established the shape: an in-crate
//! `#[cfg(test)]` module that compiles hand-built HIR with `emit_ir_only`.
//!
//! # What is asserted, and why not the old spelling
//!
//! Until #7487 the contract was three runtime calls and these tests asserted
//! the calls: `ir.contains("call i32 @js_gc_temp_root_push")`. #7487 re-lowered
//! temps onto pooled frame allocas, so those calls survive only on an FFI
//! fallback arm neither shipped lowering takes. Ten assertions in this family
//! failed and eight `!ir.contains(…)` negatives became true of every program in
//! the language — the contract had no working coverage in either direction
//! (#7503).
//!
//! Every assertion here goes through [`crate::testing::temp_slots`], which
//! names the VALUE: this producer's result went into a rooted slot, and this
//! consuming call read its operand back OUT of that slot. That is strictly
//! stronger than the call-existence check it replaces, and it is
//! lowering-independent — so each test runs **unpinned under both lowerings**
//! rather than picking one.

use crate::testing::temp_slots::{
    assert_no_temp_rooting, assert_rooted_across, first_call_result, slot_holding, slot_traffic,
    SlotEvent,
};
use crate::testing::NativeRootsPin;
use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Expr, Module, ModuleInitKind, Stmt};

mod builtin_ctor;
mod operands;

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
        fp_contract_mode: crate::FpContractMode::Off,
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
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

/// The `main` body a module-init fixture compiles into.
pub(crate) fn main_ir_for(name: &str, init: Vec<Stmt>) -> String {
    let bytes = compile_module(&module_with_init(name, init), entry_opts())
        .unwrap_or_else(|e| panic!("codegen failed for {name}: {e}"));
    let ir = String::from_utf8(bytes).expect("LLVM IR should be UTF-8");
    crate::testing::root_slots::function_slice(&ir, "main").to_string()
}

/// Run `body` once under each shipped root lowering.
///
/// Every claim in this module is about the emission contract, not about how a
/// slot is spelled, so stating it once per lowering is the honest form. It also
/// means a `PERRY_RS4GC=0` bisection and CI assert the same thing — the
/// divergence #7493 spent a week diagnosing.
pub(crate) fn under_both_lowerings(mut body: impl FnMut(&str)) {
    {
        let _pin = NativeRootsPin::native();
        body("native roots");
    }
    {
        let _pin = NativeRootsPin::shadow();
        body("shadow stack");
    }
}

/// `console.log(a, b, …)` — the shape in the #6951 repro.
pub(crate) fn console_log(args: Vec<Expr>) -> Stmt {
    Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::GlobalGet(0)),
            property: "log".to_string(),
            byte_offset: 0,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    })
}

/// An allocating argument: an object literal is a collection point, which is
/// all `expr_may_trigger_gc` needs to see.
pub(crate) fn allocating() -> Expr {
    Expr::Object(Vec::new())
}

// ---------------------------------------------------------------------------
// The argument accumulator (#6951, #6972)
// ---------------------------------------------------------------------------

/// The argument accumulator of a variadic call must live in a rooted slot, and
/// every use of it must be re-read from that slot.
///
/// Pre-fix the sequence was `js_array_alloc` → N × `js_array_push_f64` with the
/// accumulator threaded through an SSA register across each argument's
/// evaluation. That register held the ONLY reference to everything pushed so
/// far, so an allocating later argument swept the half-built array and the next
/// push landed in recycled memory — `console.log("label", churn())` lost its
/// label with no crash and no diagnostic.
///
/// Sabotage: `testing::temp_slots`'s own
/// `an_unrooted_accumulator_is_caught` / `rooted_but_not_re_read_is_caught`
/// plant both halves of the pre-fix shape and confirm this assertion rejects
/// them. Removing the store-back at the emission site fails the write-back
/// clause below in this test directly.
#[test]
fn console_argument_accumulator_is_rooted_re_read_and_written_back() {
    under_both_lowerings(|lowering| {
        let ir = main_ir_for(
            "console_accumulator.ts",
            vec![console_log(vec![
                Expr::String("label".to_string()),
                allocating(),
            ])],
        );

        let alloc = first_call_result(&ir, "js_array_alloc")
            .unwrap_or_else(|| panic!("{lowering}: no argument array was allocated:\n{ir}"));
        assert_rooted_across(&ir, &alloc, "js_array_push_f64", lowering);

        // `js_array_push_f64` may REALLOCATE, so the returned pointer has to go
        // back into the slot — otherwise the root protects the old buffer and
        // the consuming call reads a freed one. This is the clause the FFI
        // spelling expressed as `js_array_push_f64_temp_rooted`.
        let slot = slot_holding(&ir, &alloc).expect("assert_rooted_across just found it");
        let push = first_call_result(&ir, "js_array_push_f64")
            .unwrap_or_else(|| panic!("{lowering}: no push:\n{ir}"));
        // Through `slot_holding`, not an exact register compare: the store-back
        // is allowed the same one boxing step every other clause here tolerates,
        // so an exact match would fail on a lowering that boxes the push result
        // while the contract still holds (#7675 review).
        assert_eq!(
            slot_holding(&ir, &push).as_deref(),
            Some(slot.as_str()),
            "{lowering}: the reallocated array {push} must be written BACK into \
             {slot}; rooting only the pre-push pointer protects the wrong \
             allocation (#6951):\n{ir}"
        );

        // …and the consumer reads the slot, not any push's register.
        assert_rooted_across(&ir, &push, "js_console_log_spread", lowering);

        // Released after the consuming call returns, not before.
        let events = &slot_traffic(&ir)[&slot];
        let consume = ir
            .lines()
            .position(|line| line.contains("@js_console_log_spread("))
            .expect("the consumer was just asserted about");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SlotEvent::Clear { line } if *line > consume)),
            "{lowering}: the temp root must be released AFTER the consumer runs \
             (#6951):\n{ir}"
        );
    });
}

/// The gate: rooting is emitted only when something that follows can collect.
///
/// An array literal of plain literals has no allocation between its elements'
/// evaluation and the array's construction, so it must emit exactly the IR it
/// emitted before #6951 — no slot traffic, no cost.
///
/// This is one of the eight negatives that #7503 found VACUOUS: it asserted
/// `!ir.contains("call i32 @js_gc_temp_root_push")`, which since #7487 holds
/// for every program in the language. Its non-vacuity is now proved by its own
/// twin below, which must go the other way.
#[test]
fn a_non_allocating_element_list_pays_for_no_temp_root() {
    under_both_lowerings(|lowering| {
        let ir = main_ir_for(
            "array_literal_no_gc.ts",
            vec![Stmt::Expr(Expr::Array(vec![
                Expr::String("a".to_string()),
                Expr::String("b".to_string()),
                Expr::Number(1.0),
            ]))],
        );
        assert_no_temp_rooting(&ir, lowering);
    });
}

/// …and it IS emitted when an earlier element is a heap value and a later one
/// allocates: that value sits in an SSA register across the allocation, which
/// is not a root.
///
/// Structurally identical to the test above but for one element's kind, so the
/// pair is a differential: a compiler that roots nothing fails here, and one
/// that roots everything fails there.
#[test]
fn an_array_literal_roots_elements_before_an_allocating_element() {
    under_both_lowerings(|lowering| {
        let ir = main_ir_for(
            "array_literal_gc.ts",
            // Element 0 is itself a heap value (a literal would be skipped: it
            // loads from a module global that is already a registered GC root).
            vec![Stmt::Expr(Expr::Array(vec![allocating(), allocating()]))],
        );
        assert!(
            !crate::testing::temp_slots::temp_root_slots(&ir).is_empty(),
            "{lowering}: an array literal with an allocating later element must \
             root the heap elements already evaluated (#6951):\n{ir}"
        );
    });
}
