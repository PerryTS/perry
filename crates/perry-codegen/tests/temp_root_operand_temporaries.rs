//! #6969 / #6970 / #6971 — the operand temporaries that #6951 did not reach.
//!
//! #6951 rooted variadic argument accumulators, concat operand pairs and
//! literal element lists. Three sibling lowering paths kept their operands in
//! bare LLVM SSA registers across a collection point:
//!
//! - **#6970** native collection-method arguments (`m.set(fresh(), churn())`) —
//!   this one *aborted*, inside `js_map_set`, on a key whose header had been
//!   recycled;
//! - **#6969** constructor arguments, held across the instance allocation
//!   (which always collects) as well as across each other;
//! - **#6971** the string-method receiver, and `concat`'s accumulator — a bare
//!   `StringHeader*`, the form only `gc::root_words`' bare case covers.
//!
//! These tests pin the *codegen contract*. The end-to-end proof is the
//! `cons_scan_off` arm (`PERRY_CONSERVATIVE_STACK_SCAN=off`), the only
//! configuration where the bug is observable — every other automatic collection
//! forces a conservative native-stack scan that pins the temporary by accident.
//! Equally important is the negative half: the shapes that were always safe
//! must still emit no rooting calls at all.
//!
//! LOWERING (#7493). Two tests here assert on the SHADOW-STACK spelling of the
//! `this`-slot root (`js_shadow_slot_bind`) and pin `NativeRootsPin::shadow()`;
//! `a_collection_free_construction_emits_no_this_slot_root` needed the pin even
//! though it was *passing*, because under the post-#7370 native-roots default
//! its `!contains("@js_shadow_slot_bind")` is true of every program (hazard 4:
//! the gate ran, its subject did not). Since #7876 every class-key cache also
//! has one function-lifetime bind; that negative asserts there is exactly that
//! one bind rather than no bind anywhere in the function.
//!
//! The rest of this file is lowering-INDEPENDENT and deliberately unpinned.
//!
//! SPELLING (#7503). Until #7487 the temp-root contract was three runtime calls
//! and this file asserted the calls. #7487 re-lowered temp roots onto pooled
//! frame allocas — push became a store, get a load, truncate a slot clear — so
//! `js_gc_temp_root_push` / `_get` / `_set` / `_truncate` now survive only on an
//! FFI fallback arm that neither shipped lowering takes. Six positive
//! assertions here failed, and — worse — every
//! `!ir.contains("call i32 @js_gc_temp_root_push")` NEGATIVE held for every
//! program in the language, rooted or not. The contract itself never changed.
//!
//! Every rooting claim now goes through `perry_codegen::testing::temp_slots`,
//! which names the VALUE — this producer's result went into a rooted slot, and
//! this consuming call read it back OUT of that slot — and understands both
//! lowerings' spelling of a slot. That is strictly stronger than the call-
//! existence check it replaces: `contains("…temp_root_push")` only ever proved
//! that SOME call existed SOMEWHERE in the module.
//!
//! The argument-accumulator half of this family lives in
//! `perry_codegen::temp_root_coverage` since #6988 — in `src/`, so it runs in
//! the per-PR `cargo-test` gate rather than the nightly-only integration tier.

use perry_codegen::testing::temp_slots::{
    assert_no_temp_rooting, assert_rooted_across, derives_from_slot_load, first_call_result,
    slot_holding, slot_traffic, temp_root_slots, SlotEvent,
};
use perry_codegen::testing::NativeRootsPin;
use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Class, Expr, Module, ModuleInitKind, Stmt};

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
        short_spread_method_candidates: std::sync::Arc::default(),
        object_literal_method_candidates: std::sync::Arc::default(),
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
        classic_for_lexical_bindings: std::collections::HashSet::new(),
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

fn ir_for(name: &str, init: Vec<Stmt>) -> String {
    String::from_utf8(compile_module(&module_with_init(name, init), entry_opts()).unwrap())
        .expect("LLVM IR should be UTF-8")
}

/// An allocating operand: an object literal is a collection point, which is all
/// `expr_may_trigger_gc` needs to see.
fn allocating() -> Expr {
    Expr::Object(Vec::new())
}

// ---------------------------------------------------------------- #6970 ----

/// `m.set(key, value)` where `value` allocates: `key` is finished but lives in
/// an SSA register across `value`'s lowering.
///
/// This is the abort in #6970. `js_map_set` ran with a key pointer whose block
/// the sweep had already returned and `churn` had reused, and the Map's
/// side-allocation owner record no longer matched — `grown Map must retain its
/// side-allocation owner record`, exit 134.
#[test]
fn map_set_key_is_rooted_across_an_allocating_value() {
    let ir = ir_for(
        "map_set_rooted.ts",
        vec![Stmt::Expr(Expr::MapSet {
            map: Box::new(Expr::MapNew),
            key: Box::new(allocating()),
            value: Box::new(allocating()),
        })],
    );

    let f = init_ir(&ir);
    let key = first_call_result(f, "js_object_alloc")
        .unwrap_or_else(|| panic!("the key must allocate, or this proves nothing:\n{f}"));
    assert_rooted_across(f, &key, "js_map_set", "#6970 map.set key");

    let slot = slot_holding(f, &key).expect("assert_rooted_across just found the slot");
    let consume = f
        .lines()
        .position(|line| line.contains("@js_map_set("))
        .expect("assert_rooted_across just found the call");
    assert!(
        slot_traffic(f)[&slot]
            .iter()
            .any(|e| matches!(e, SlotEvent::Clear { line } if *line > consume)),
        "the release comes AFTER the consuming call, because js_map_set \
         allocates while it reads the key:\n{f}"
    );
}

/// The gate: a `map.set` whose value cannot collect must emit no rooting at all.
#[test]
fn map_set_with_a_non_allocating_value_emits_no_rooting_calls() {
    let ir = ir_for(
        "map_set_no_gc.ts",
        vec![Stmt::Expr(Expr::MapSet {
            map: Box::new(Expr::MapNew),
            key: Box::new(Expr::String("k".to_string())),
            value: Box::new(Expr::Number(1.0)),
        })],
    );

    assert_no_temp_rooting(
        init_ir(&ir),
        "#6970 gate: nothing after the key can collect, so this must cost \
         exactly what it cost before",
    );
}

// ---------------------------------------------------------------- #6971 ----

/// `s.concat(x)` used to thread a bare `StringHeader*` accumulator through an
/// SSA register across every argument, written back into its own slot after
/// each `js_string_concat` call because every call returns a NEW address.
///
/// #8450 replaced that iterative accumulator with a fused
/// `js_string_concat_chain` call: the receiver and every argument are each
/// rooted in their OWN slot (not one shared, repeatedly-written slot), then
/// re-read into a scratch array right before the single chain call. This
/// pins the #8450-shaped invariant directly with `assert_rooted_across`
/// rather than counting stores into a shared accumulator that no longer
/// exists.
#[test]
fn concat_accumulator_is_rooted_and_written_back() {
    let ir = ir_for(
        "concat_rooted.ts",
        vec![Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::Binary {
                    op: perry_hir::BinaryOp::Add,
                    left: Box::new(Expr::String("a".to_string())),
                    right: Box::new(Expr::String("b".to_string())),
                }),
                property: "concat".to_string(),
                byte_offset: 0,
            }),
            args: vec![allocating()],
            type_args: Vec::new(),
            byte_offset: 0,
        })],
    );

    let f = init_ir(&ir);
    let arg = first_call_result(f, "js_object_alloc").unwrap_or_else(|| {
        panic!("the concat argument must allocate, or this proves nothing:\n{f}")
    });
    let slot = slot_holding(f, &arg).unwrap_or_else(|| {
        panic!(
            "the concat argument must be rooted across evaluation of later \
             parts (#6971/#8450):\n{f}"
        )
    });
    let chain_line = f
        .lines()
        .position(|line| line.contains("@js_string_concat_chain("))
        .expect("this test's whole point is a js_string_concat_chain call");
    // #8450 fuses the receiver + args into a scratch array passed by pointer,
    // so the call's own operands are just `(ptr, i32)` — the rooted re-read
    // shows up one level removed, in the `store double %v, ptr %slot` that
    // fills the array. Any of those stored values deriving from a load out of
    // the rooted slot proves the fused call consumes the re-read, not the
    // stale producer register.
    let fed_from_reread = f
        .lines()
        .take(chain_line)
        .filter_map(|line| line.trim().strip_prefix("store double ")?.split(',').next())
        .any(|reg| derives_from_slot_load(f, reg.trim(), 6));
    assert!(
        fed_from_reread,
        "the concat argument (rooted in {slot}) must be re-read before it is \
         stored into the js_string_concat_chain scratch array — otherwise the \
         array holds the stale pre-collection register (#6971/#8450):\n{f}"
    );
}

/// The gate for the whole string-method family: a receiver whose arguments
/// cannot collect must not pay for the dispatch-wide root.
#[test]
fn string_method_with_non_allocating_args_emits_no_rooting_calls() {
    let ir = ir_for(
        "string_method_no_gc.ts",
        vec![Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::Binary {
                    op: perry_hir::BinaryOp::Add,
                    left: Box::new(Expr::String("a".to_string())),
                    right: Box::new(Expr::String("b".to_string())),
                }),
                property: "slice".to_string(),
                byte_offset: 0,
            }),
            args: vec![Expr::Number(1.0)],
            type_args: Vec::new(),
            byte_offset: 0,
        })],
    );

    assert_no_temp_rooting(
        init_ir(&ir),
        "#6971 gate: a numeric argument cannot collect, so the receiver needs \
         no root",
    );
}

// ---------------------------------------------------------------- #6969 ----

/// A module declaring `class Pair {}` plus a top-level `new Pair(a, b)`.
fn module_with_new(name: &str, args: Vec<Expr>) -> Module {
    let mut module = module_with_init(
        name,
        vec![Stmt::Expr(Expr::New {
            class_name: "Pair".to_string(),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        })],
    );
    module.classes = vec![Class {
        id: 1,
        name: "Pair".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }];
    module
}

fn ir_for_new(name: &str, args: Vec<Expr>) -> String {
    String::from_utf8(compile_module(&module_with_new(name, args), entry_opts()).unwrap())
        .expect("LLVM IR should be UTF-8")
}

/// Constructor arguments are all lowered before the instance is allocated, and
/// that allocation always collects — so every heap-valued argument must be
/// rooted, and re-read after the allocation.
///
/// The rooting must also be interleaved with the lowering, not appended after
/// it: argument 0 is live across argument 1's evaluation. Pushing the whole
/// list afterwards is strictly worse than not rooting at all — it publishes an
/// already-dangling pointer to the scanner, which turned the #6969 silent DIFF
/// into a SIGSEGV while this fix was being written.
#[test]
fn constructor_arguments_are_rooted_across_the_instance_allocation() {
    let ir = ir_for_new("ctor_args_rooted.ts", vec![allocating(), allocating()]);

    let f = init_ir(&ir);
    let traffic = slot_traffic(f);

    // Each argument is an object literal, so each lowers to its own
    // `js_object_alloc`; the SECOND one is argument 1's, i.e. the collection
    // point argument 0 has to survive.
    let arg_allocs: Vec<(usize, String)> = f
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("call i64 @js_object_alloc("))
        .filter_map(|(i, l)| l.trim().split_once(" = ").map(|(r, _)| (i, r.to_string())))
        .collect();
    assert_eq!(arg_allocs.len(), 2, "both arguments allocate:\n{f}");

    let (arg1_line, _) = &arg_allocs[1];
    let (_, arg0) = &arg_allocs[0];
    let slot = slot_holding(f, arg0)
        .unwrap_or_else(|| panic!("constructor argument 0 ({arg0}) is never rooted (#6969):\n{f}"));
    let arg0_store = traffic[&slot]
        .iter()
        .find_map(|e| match e {
            SlotEvent::Store { line, .. } => Some(*line),
            _ => None,
        })
        .expect("slot_holding just found it");
    assert!(
        arg0_store < *arg1_line,
        "argument 0 must be rooted BEFORE argument 1 is lowered — rooting the \
         whole list after the loop publishes an already-dangling pointer \
         (#6969). Store at {arg0_store}, argument 1 allocates at {arg1_line}:\n{f}"
    );

    // …and every argument is still rooted across the INSTANCE allocation, then
    // re-read below it. That call is the one that always collects.
    let instance_alloc = f
        .lines()
        .position(|l| l.contains("call i64 @js_object_alloc_class_inline_keys"))
        .unwrap_or_else(|| panic!("the instance allocation:\n{f}"));
    assert!(
        traffic
            .values()
            .flat_map(|events| events.iter())
            .any(|e| matches!(e, SlotEvent::Load { line, .. } if *line > instance_alloc)),
        "the arguments must be re-read from their slots AFTER the instance \
         allocation — a register held across it names from-space (#6969):\n{f}"
    );
    // The scope cut comes after the arguments are consumed — per SLOT. A
    // module-wide "last load before any clear" would be answered by an
    // unrelated named local's slot, which is the class of mistake this whole
    // issue is about.
    for slot in temp_root_slots(f) {
        let events = &traffic[&slot];
        let last_load = events
            .iter()
            .filter_map(|e| match e {
                SlotEvent::Load { line, .. } => Some(*line),
                _ => None,
            })
            .max()
            .unwrap_or_else(|| panic!("{slot} was rooted and never read:\n{f}"));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SlotEvent::Clear { line } if *line > last_load)),
            "{slot}'s scope cut must come after its last read (line {last_load}); \
             releasing early un-roots a value still in use:\n{f}"
        );
    }
}

/// The gate: `new Pair(a, b)` on immediates must emit no rooting.
///
/// A number roots nothing at all, and a string literal is already a registered
/// root, so neither needs a temp-root slot. (The literal is still *re-loaded*
/// after the allocation — see
/// `registered_root_operands_are_reloaded_rather_than_rooted` — but that is a
/// plain load, not a runtime call.)
#[test]
fn constructor_arguments_on_plain_locals_emit_no_rooting_calls() {
    let ir = ir_for_new(
        "ctor_args_locals.ts",
        vec![Expr::Number(1.0), Expr::String("s".to_string())],
    );

    assert_no_temp_rooting(
        init_ir(&ir),
        "#6969 gate: a number and a string literal need no root — the literal \
         is a load from a module global already registered with \
         js_gc_register_global_root",
    );
}

/// An operand that reads a *registered root* is not rooted again — but it must
/// be **re-loaded**, not reused from its pre-collection register.
///
/// A string literal, a module global and a shadow-slotted local are all marked
/// by the collector, so they are never swept. But an evacuating cycle
/// **rewrites their storage**, and the register loaded before the collection
/// still holds the pre-move address. Emitting the load again is correct and
/// costs no runtime call — the same staleness #6981 reports one layer in, for a
/// raw typed-array pointer under the specialized ABI.
#[test]
fn registered_root_operands_are_reloaded_rather_than_rooted() {
    let ir = ir_for_new(
        "ctor_args_reload.ts",
        vec![Expr::String("lit".to_string()), allocating()],
    );

    let handle_load = "load double, ptr @ctor_args_reload_ts_.str.";
    let loads: Vec<usize> = ir.match_indices(handle_load).map(|(i, _)| i).collect();
    assert!(
        loads.len() >= 2,
        "the literal operand must be loaded a SECOND time after the instance \
         allocation — reusing the first register leaves it pointing at where \
         the string used to be once evacuation moves it:\n{ir}"
    );

    let alloc = ir
        .find("call i64 @js_object_alloc_class_inline_keys")
        .expect("the instance allocation");
    assert!(
        loads[0] < alloc && loads.iter().any(|&l| l > alloc),
        "one load before the allocation (the original lowering) and one after \
         it (the re-load):\n{ir}"
    );

    // And it must be a re-LOAD, not a temp root: a registered root needs no
    // second liveness mechanism, so this must cost zero slot traffic for it.
    // The allocating operand still gets a real root — so the count is exactly
    // one, and "at most one" would let a regression that roots the literal but
    // stops rooting the object pass.
    let slots = temp_root_slots(init_ir(&ir));
    assert_eq!(
        slots.len(),
        1,
        "expected exactly the allocating operand's root; the literal must not \
         get a slot of its own, and the object must not lose one. Slots: \
         {slots:?}\n{}",
        init_ir(&ir)
    );
}

// ---------------------------------------------------------------- #7114 ----
//
// The other half of the same claim, and the one that was missing. `new C(...)`
// (above) suppressed the string literal AND re-loaded it. `lower_exprs_rooted`
// — the helper behind `lower_operand_pair_rooted`, the array-literal element
// list and the string-concat chain — suppressed it and reused the register.
//
// So `console.log("acc:" + run(1e7))` loaded the literal's handle before the
// call and masked the cached register to a pointer after it. The handle global
// is a registered root that evacuation rewrites, the register is not, and the
// concat read the string's pre-move address: an empty line, exit code 0.

/// An operand that is statically NUMERIC *and* can collect, so `"lit" + it`
/// takes the fused `js_string_concat_value_box` path — the exact expression form
/// #7114 was reported against.
///
/// A non-`Add` `Expr::Binary` is numeric by construction (`is_numeric_expr`),
/// and it is GC-capable whenever an operand is not an inert primitive, which an
/// object literal is not. That is the same pairing as the reported repro, where
/// the sibling was a `number`-returning call whose body allocated.
fn allocating_numeric() -> Expr {
    Expr::Binary {
        op: perry_hir::BinaryOp::Sub,
        left: Box::new(allocating()),
        right: Box::new(Expr::Number(0.0)),
    }
}

/// The IR of ONE generated function, sliced out of the module.
///
/// `ir_for` / `ir_for_new` return the **whole module**, and an assertion about
/// instruction ORDER over a whole module is satisfiable by an unrelated
/// function's IR: `ir.find("call i64 @js_object_alloc(")` answers with whichever
/// function happens to come first in the file, not with the operand under test.
/// A test that passes that way has proved nothing, which is exactly what it was
/// written to rule out (raised on #7116).
///
/// Every ordering assertion in the #7114 section goes through here, and pairs
/// the ordering with an exact *count* of the operand-specific opcode inside the
/// slice — order plus count is what makes "this is the operand I meant" a
/// property of the test rather than of the module layout.
fn function_ir<'a>(ir: &'a str, define_prefix: &str) -> &'a str {
    let start = ir
        .find(define_prefix)
        .unwrap_or_else(|| panic!("no `{define_prefix}` in module IR:\n{ir}"));
    let body = &ir[start..];
    let end = body.find("\n}\n").map(|e| e + 2).unwrap_or(body.len());
    &body[..end]
}

/// The function a `module_with_init` / `module_with_new` statement lowers into.
/// Module-level `init` statements are emitted into the entry module's `@main`.
fn init_ir(ir: &str) -> &str {
    function_ir(ir, "define i32 @main(")
}

/// The scoping itself, made falsifiable.
///
/// Everything below rests on `init_ir` genuinely narrowing the search. If it
/// silently returned the whole module the ordering assertions would go back to
/// being satisfiable by an unrelated function's IR and nothing else in this file
/// would notice — so the narrowing is asserted directly: the slice is one
/// function, strictly smaller than the module, and it does NOT reach
/// `__perry_init_strings_*`, which is the other function in these modules that
/// touches the same handle globals.
#[test]
fn init_ir_slices_only_the_function_under_test() {
    let ir = ir_for(
        "scope_check.ts",
        vec![Stmt::Expr(Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left: Box::new(Expr::String("acc:".to_string())),
            right: Box::new(allocating_numeric()),
        })],
    );
    let f = init_ir(&ir);

    assert!(
        f.starts_with("define i32 @main("),
        "the slice must begin at the function it names:\n{f}"
    );
    assert_eq!(
        f.matches("\ndefine ").count(),
        0,
        "and end before the next one — exactly one function in the slice:\n{f}"
    );
    assert!(
        f.len() < ir.len(),
        "a slice the size of the module is not a slice"
    );
    assert!(
        ir.contains("define void @__perry_init_strings_"),
        "precondition: the module really does contain another function that \
         touches the same handle globals, otherwise this test proves nothing"
    );
    assert!(
        !f.contains("__perry_init_strings_scope_check_ts_chunk"),
        "and the slice must not reach it (#7116):\n{f}"
    );
}

/// `"acc:" + <allocating numeric>` — the reported shape.
///
/// The invariant: **no operand register may outlive a collection point.** The
/// literal is not rooted (it does not need to be — it is already a registered
/// root and can never be swept), so the only thing that makes it correct is
/// that its `load` is emitted BELOW the sibling that collects.
#[test]
fn string_literal_concat_operand_is_re_derived_below_the_allocating_sibling() {
    let ir = ir_for(
        "concat_reload.ts",
        vec![Stmt::Expr(Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left: Box::new(Expr::String("acc:".to_string())),
            right: Box::new(allocating_numeric()),
        })],
    );

    // Scoped to @main, so nothing below can be satisfied by another function's
    // IR, and count-anchored, so "the concat" and "the sibling's allocation"
    // are unambiguous rather than "whichever matched first".
    let f = init_ir(&ir);
    let handle = "load double, ptr @concat_reload_ts_.str.";
    assert_eq!(
        f.matches("call double @js_string_concat_value_box(")
            .count(),
        1,
        "exactly one fused string+value concat in @main:\n{f}"
    );
    assert_eq!(
        f.matches("call i64 @js_object_alloc(").count(),
        1,
        "exactly one allocating sibling in @main:\n{f}"
    );
    assert_eq!(
        f.matches(handle).count(),
        2,
        "the literal's handle must be loaded TWICE in @main: once where the \
         operand is lowered, and once re-derived below the collection point. \
         One load is the #7114 bug; three means something else is re-lowering \
         it:\n{f}"
    );

    let concat = f.find("call double @js_string_concat_value_box(").unwrap();
    let alloc = f[..concat]
        .rfind("call i64 @js_object_alloc(")
        .unwrap_or_else(|| panic!("the sibling must allocate before the concat:\n{f}"));
    let handle_load = f[..concat]
        .rfind(handle)
        .unwrap_or_else(|| panic!("the literal must come from its handle global:\n{f}"));

    assert!(
        handle_load > alloc,
        "#7114: the handle load that feeds the concat must sit BELOW the \
         allocating sibling. Loading it above and masking the cached register \
         below is what made `console.log(\"acc:\" + run(1e7))` print an empty \
         line — the handle global is a registered root that an evacuating \
         cycle REWRITES, and the pre-call register keeps the pre-move \
         address:\n{f}"
    );

    assert_no_temp_rooting(
        f,
        "#7114: a registered root already has liveness; all it was missing is \
         the re-derivation, which is the load that was going to be emitted \
         anyway — so it must cost no slot",
    );
}

/// The gate. Nothing after the literal can collect, so nothing may move, so the
/// register is still the value the call observes — and the IR must be exactly
/// what it was before #6951 and before #7114: ONE load, no second one.
///
/// Without this half the "fix" could be an unconditional re-load, which would
/// pay for the reported bug on every `"user_" + i` in the codebase.
#[test]
fn string_literal_concat_operand_is_not_re_derived_when_nothing_collects() {
    let ir = ir_for(
        "concat_noreload.ts",
        vec![Stmt::Expr(Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left: Box::new(Expr::String("acc:".to_string())),
            right: Box::new(Expr::Compare {
                op: perry_hir::CompareOp::Lt,
                left: Box::new(Expr::Number(1.0)),
                right: Box::new(Expr::Number(2.0)),
            }),
        })],
    );

    let f = init_ir(&ir);
    assert_eq!(
        f.matches("load double, ptr @concat_noreload_ts_.str.")
            .count(),
        1,
        "a comparison over two immediates runs no user code and allocates \
         nothing, so the literal must be loaded exactly once:\n{f}"
    );
    assert_no_temp_rooting(f, "#7114 gate: nothing collects, so no rooting at all");
}

/// The same helper, reached from its other caller: an array literal's element
/// list. `["lit", allocating()]` holds the literal across the element that
/// collects, and the array's own store must receive the re-derived address.
#[test]
fn string_literal_array_element_is_re_derived_below_an_allocating_element() {
    let ir = ir_for(
        "array_reload.ts",
        vec![Stmt::Expr(Expr::Array(vec![
            Expr::String("lit".to_string()),
            allocating(),
        ]))],
    );

    let f = init_ir(&ir);
    let handle_pat = "load double, ptr @array_reload_ts_.str.";
    assert_eq!(
        f.matches("call i64 @js_object_alloc(").count(),
        1,
        "exactly one allocating element in @main:\n{f}"
    );
    let element_alloc = f
        .lines()
        .position(|l| l.contains("call i64 @js_object_alloc("))
        .expect("just counted it");

    // #7114's invariant admits TWO discharges, and which one a given operand
    // gets is a lowering decision, not part of the contract: re-derive the
    // value from immutable storage below the collection point, or park it in a
    // rooted slot and re-read it from there. Asserting only the first is what
    // made this test fail on a compiler that had switched to the second. What
    // must never happen is the third thing: reusing the pre-collection
    // register.
    let re_derived = f
        .lines()
        .enumerate()
        .any(|(i, l)| i > element_alloc && l.contains(handle_pat));
    let re_read = slot_traffic(f)
        .values()
        .flat_map(|events| events.iter())
        .any(|e| matches!(e, SlotEvent::Load { line, .. } if *line > element_alloc));
    assert!(
        re_derived || re_read,
        "#7114: element 0 must reach the array store either re-derived from its \
         handle global BELOW element 1's allocation, or re-read from a rooted \
         slot below it. Neither happened, so the value stored is the \
         pre-relocation address. Slot traffic: {:#?}\n{f}",
        slot_traffic(f)
    );
}

/// The sibling literal forms, and why the asymmetry in `operand_is_reloadable`
/// is deliberate rather than an oversight (raised on #7116).
///
/// A WTF-8 literal — a lone surrogate — lowers to exactly the same thing as
/// `Expr::String`: one load of a `__perry_init_strings_*` handle global that
/// `js_gc_register_global_root` registered. It is nonetheless **not** in the
/// suppression list, so it takes a real temp root rather than a re-load, and
/// that is strictly stronger: `Root` supplies liveness, a rewritten location
/// and the call-time value on its own.
///
/// The hazard worth gating is not the asymmetry but *half*-closing it. Adding a
/// literal form to `operand_needs_root`'s suppression list without also adding
/// it to `operand_is_reloadable` drops it to `Reuse` — #7114, for that form —
/// and no other test in this file would notice. This one goes red.
#[test]
fn wtf8_literal_operand_is_rooted_not_merely_reused() {
    // 0xED 0xA0 0x80 is U+D800 in WTF-8: a lone surrogate, which is what routes
    // a literal to `Expr::WtfString` instead of `Expr::String`.
    let ir = ir_for(
        "wtf8_operand.ts",
        vec![Stmt::Expr(Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left: Box::new(Expr::WtfString(vec![0xED, 0xA0, 0x80])),
            right: Box::new(allocating_numeric()),
        })],
    );

    let f = init_ir(&ir);
    let slots = temp_root_slots(f);
    assert_eq!(
        slots.len(),
        1,
        "exactly one temp root in @main — the WTF-8 operand's. Zero means it \
         was suppressed without a compensating re-derivation (#7114 for \
         lone-surrogate literals); more than one means this assertion is no \
         longer about the operand it names. Slots: {slots:?}\n{f}"
    );
    assert_eq!(
        f.matches("call i64 @js_object_alloc(").count(),
        1,
        "exactly one allocating sibling in @main:\n{f}"
    );

    let alloc = f
        .lines()
        .position(|l| l.contains("call i64 @js_object_alloc("))
        .expect("just counted it");
    let events = &slot_traffic(f)[&slots[0]];
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SlotEvent::Store { line, .. } if *line < alloc)),
        "the WTF-8 literal must reach its slot BEFORE the allocating sibling \
         runs:\n{f}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SlotEvent::Load { line, .. } if *line > alloc)),
        "…and be re-read AFTER it. Anything else means the literal is being \
         carried across the collection point in a register, which is #7114 for \
         lone-surrogate literals:\n{f}"
    );
}

// ---------------------------------------------------------------- #7154 ----

/// `Pair` with an instance field, so construction runs user code (the field
/// initializer) between the allocation and the value `new` yields.
fn module_with_new_running_ctor(name: &str) -> Module {
    let mut module = module_with_new(name, Vec::new());
    module.classes[0].fields = vec![perry_hir::ClassField {
        name: "v".to_string(),
        key_expr: None,
        ty: perry_hir::types::Type::Any,
        // An object literal: a real collection point inside the constructor.
        init: Some(Expr::Object(Vec::new())),
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }];
    module
}

/// #7154, the sibling of #7184: the freshly-allocated instance must be ROOTED
/// across the constructor body and RE-READ afterwards.
///
/// The constructor body allocates, and under `PERRY_GC_MOVING_LOOP_POLLS=1` a
/// back-edge poll inside it drives an evacuating minor. The instance survives —
/// the callee's own `this` shadow slot roots it — which means it *moves*, and
/// the collector rewrites the callee's root but not the caller's SSA register.
/// Everything downstream in the caller (`js_gc_init_typed_shape_layout`, the
/// capture write-back, `js_ctor_return_override`) then names from-space memory,
/// and the override publishes that dead address into the caller's shadow slot:
/// a *rooted* slot holding a dangling pointer, read back later as
/// "TypeError: value is not a function".
///
/// Sabotage check: drop the `reload_instance` call in `lower_new_impl_inner`
/// and the re-read disappears from between the allocation and the override.
#[test]
fn the_new_instance_is_rooted_across_the_constructor_body() {
    let ir = String::from_utf8(
        compile_module(
            &module_with_new_running_ctor("new_inst_rooted.ts"),
            entry_opts(),
        )
        .unwrap(),
    )
    .expect("LLVM IR should be UTF-8");
    let f = init_ir(&ir);

    // Assertions are on the DEF-USE chain, not on textual order: the override
    // is emitted into the `ctor.return.after` block, which the writer appends
    // below the block that re-reads the root.

    // 1. The allocation's result reaches a rooted slot immediately.
    let inst_reg = f
        .lines()
        .find(|l| l.contains("call i64 @js_object_alloc_class"))
        .and_then(|l| l.trim().split_once(" = ").map(|(r, _)| r.to_string()))
        .unwrap_or_else(|| panic!("the instance allocation:\n{f}"));
    assert!(
        slot_holding(f, &inst_reg).is_some(),
        "the instance {inst_reg} must be rooted as soon as it is allocated, \
         before the constructor body runs (#7154). Slot traffic: {:#?}\n{f}",
        slot_traffic(f)
    );

    // 2. The value `js_ctor_return_override` publishes is re-read from that
    //    root, not carried across the constructor in the original register.
    let override_line = f
        .lines()
        .find(|l| l.contains("call double @js_ctor_return_override"))
        .unwrap_or_else(|| panic!("the return-override:\n{f}"));
    let published = override_line
        .split_once("js_ctor_return_override(")
        .expect("argument list")
        .1
        .split(',')
        .next()
        .and_then(|a| a.trim().rsplit_once(' ').map(|(_, r)| r.to_string()))
        .unwrap_or_else(|| panic!("no register operand in `{override_line}`"));
    assert!(
        derives_from_slot_load(f, &published, 8),
        "the instance handed to js_ctor_return_override ({published}) must be \
         re-read from its root after the constructor body — the callee's own \
         `this` slot keeps it alive, so it MOVES, and the caller's register \
         names from-space (#7154):\n{f}"
    );
}

/// The gate: a class with no constructor, no fields and no heritage runs no
/// user code between the allocation and the `new` value, so it must keep its
/// pre-#7154 IR — no instance root, and no scope marker either.
#[test]
fn a_class_that_runs_no_user_code_emits_no_instance_root() {
    let ir = ir_for_new("new_inst_no_ctor.ts", vec![Expr::Number(1.0)]);
    assert_no_temp_rooting(
        init_ir(&ir),
        "#7154 gate: nothing can collect between the allocation and the `new` \
         value, so rooting the instance would be pure cost",
    );
}

/// #7154 follow-up: the inline-constructor **result slot** must not be seeded
/// with the instance address.
///
/// `ctor_result_slot` is a plain entry alloca — not a shadow slot, not a temp
/// root — so the collector neither marks nor rewrites it. Seeding it with the
/// pre-constructor `obj_box` meant that on fall-through (no explicit `return`)
/// `js_ctor_return_override` received an *object* in `raw` and returned THAT,
/// discarding the re-read instance the reload had just recovered. The
/// dominance fix was defeated at its last instruction, and
/// `the_new_instance_is_rooted_across_the_constructor_body` could not see it:
/// that test walks the FIRST operand, which is the re-read one.
///
/// Sabotage check: restore the `store double %obj_box, ptr %ctor_result_slot`
/// seed in `lower_new_impl_inner` and this fails.
#[test]
fn the_inline_ctor_result_slot_never_carries_an_instance_address() {
    let ir = String::from_utf8(
        compile_module(
            &module_with_new_running_ctor("new_inst_result_slot.ts"),
            entry_opts(),
        )
        .unwrap(),
    )
    .expect("LLVM IR should be UTF-8");
    let f = init_ir(&ir);

    let override_line = f
        .lines()
        .find(|l| l.contains("call double @js_ctor_return_override"))
        .unwrap_or_else(|| panic!("the return-override:\n{f}"));
    // `call double @js_ctor_return_override(double %a, double %b, i32 N)` —
    // `%b` is `raw`, the value loaded out of the result slot.
    let args = override_line
        .split_once("js_ctor_return_override(")
        .expect("argument list")
        .1;
    let raw_reg = args
        .split(", ")
        .nth(1)
        .and_then(|a| a.trim().strip_prefix("double %"))
        .unwrap_or_else(|| panic!("no `raw` register operand in `{override_line}`"))
        .trim_end_matches(')')
        .to_string();

    let raw_def = f
        .lines()
        .find(|l| l.trim_start().starts_with(&format!("%{raw_reg} = ")))
        .unwrap_or_else(|| panic!("no definition of %{raw_reg} in:\n{f}"))
        .to_string();
    assert!(
        raw_def.contains("load double"),
        "`raw` should be a load from the inline-ctor result slot, got `{raw_def}`:\n{f}"
    );
    let slot = raw_def
        .rsplit_once("ptr ")
        .expect("the slot pointer operand")
        .1
        .trim()
        .to_string();

    // Every store into that slot must be a constant. A register operand means
    // an address the collector cannot rewrite is sitting in unrooted memory
    // across the constructor body.
    for line in f
        .lines()
        .filter(|l| l.trim_end().ends_with(&format!("ptr {slot}")) && l.contains("store "))
    {
        let stored = line
            .split_once("store double ")
            .map(|(_, rest)| rest.split(',').next().unwrap_or("").trim().to_string())
            .unwrap_or_default();
        assert!(
            !stored.starts_with('%'),
            "the inline-ctor result slot is a plain alloca the collector does \
             not rewrite, so it must never be seeded with a heap address — \
             found `{line}` (#7154):\n{f}"
        );
    }
}

/// #7202: the inline-constructor `this` slot must be a **rewritten** root.
///
/// `lower_call/new.rs` allocates `this_slot` with `alloca_entry` and every
/// `this` read inside the inlined body is a `load` from it. A plain
/// `alloca_entry` is neither a shadow slot nor a temp root, so an evacuating
/// minor at a field initializer's back-edge poll neither marks nor rewrites it,
/// and every `this.x = …` after that collection stores into abandoned
/// from-space memory.
///
/// #7192 rooted the instance for the CALLER (`instance_root`, re-read by
/// `reload_instance`) precisely because this window collects — so the object
/// survives and MOVES. That made the caller's copy correct and left this one
/// behind: the same address, taken one line later, that nothing rewrites.
///
/// Binding the alloca is the fix rather than routing `Expr::This` through a
/// temp root, because ~30 readers already `load` from it and
/// `js_shadow_slot_bind` records `slot_ptrs[idx] = alloca`, so evacuation
/// rewrites it in place.
///
/// Sabotage check: drop the `root_entry_alloca` call in `lower_new_impl_inner`
/// and no bind names the `this` slot.
#[test]
fn the_inline_ctor_this_slot_is_bound_as_a_shadow_slot() {
    let _pin = NativeRootsPin::shadow();
    let ir = String::from_utf8(
        compile_module(
            &module_with_new_running_ctor("new_inst_this_slot.ts"),
            entry_opts(),
        )
        .unwrap(),
    )
    .expect("LLVM IR should be UTF-8");
    let f = init_ir(&ir);

    // `this` is read by the field-initializer store; find the slot it loads
    // from by taking the alloca that a `js_shadow_slot_bind` names AND that is
    // stored with a nanboxed instance. Simpler and stronger: assert that every
    // `double` alloca which receives a register store and is later loaded is
    // covered by a bind.
    let bound: std::collections::HashSet<String> = f
        .lines()
        .filter_map(|l| l.split_once("js_shadow_slot_bind(i32 "))
        .filter_map(|(_, rest)| rest.split_once("ptr ").map(|(_, p)| p))
        .map(|p| p.trim().trim_end_matches(')').to_string())
        .collect();
    assert!(
        !bound.is_empty(),
        "expected at least one js_shadow_slot_bind in:\n{f}"
    );

    // The instance's nanbox register: walk each `store double %X, ptr %S`
    // backwards through the bit-level nanbox ops to see whether `%X` was
    // produced by an object allocation. That is the `this` slot.
    let def_of: std::collections::HashMap<&str, &str> = f
        .lines()
        .filter_map(|l| l.trim_start().split_once(" = "))
        .map(|(r, rhs)| (r.trim(), rhs))
        .collect();
    fn reaches_alloc(
        reg: &str,
        def_of: &std::collections::HashMap<&str, &str>,
        depth: usize,
    ) -> bool {
        if depth == 0 {
            return false;
        }
        let Some(rhs) = def_of.get(reg) else {
            return false;
        };
        if rhs.contains("@js_object_alloc") {
            return true;
        }
        // Only follow bit-level identity producers, exactly as the dominance
        // checker's `provenance` does; anything else is a different value.
        if !(rhs.starts_with("or i64")
            || rhs.starts_with("bitcast")
            || rhs.starts_with("inttoptr")
            || rhs.starts_with("ptrtoint"))
        {
            return false;
        }
        rhs.split(|c: char| !(c.is_alphanumeric() || c == '%' || c == '.' || c == '_'))
            .filter(|w| w.starts_with('%'))
            .any(|w| reaches_alloc(w, def_of, depth - 1))
    }

    let this_slot = f
        .lines()
        .filter(|l| l.trim_start().starts_with("store double %"))
        .find_map(|l| {
            let (val, rest) = l
                .trim_start()
                .strip_prefix("store double ")?
                .split_once(", ")?;
            let slot = rest.strip_prefix("ptr ")?.trim();
            reaches_alloc(val.trim(), &def_of, 8).then(|| slot.to_string())
        })
        .unwrap_or_else(|| panic!("no store of a freshly allocated instance into a slot in:\n{f}"));

    assert!(
        bound.contains(&this_slot),
        "the inline-ctor `this` slot {this_slot} is a plain entry alloca that \
         the collector neither marks nor rewrites, yet it holds the instance \
         across the whole constructor body — it must be bound as a shadow slot \
         (#7202). Bound slots: {bound:?}\n{f}"
    );

    // The bind contract also requires an `undefined` seed: the bind is hoisted
    // to entry setup, so the collector dereferences the alloca before the
    // instance store executes.
    assert!(
        f.lines().any(|l| {
            l.trim_start()
                .starts_with("store double 0x7FFC000000000001")
                && l.trim_end().ends_with(&format!("ptr {this_slot}"))
        }),
        "the `this` slot must be seeded with `undefined` in entry_allocas \
         before the hoisted bind makes it live to the collector (#7202/#6968) \
         — no such store for {this_slot}:\n{f}"
    );
}

/// #7200: `Object.assign(t, …sources)` threads its accumulator through a bare
/// SSA register across every source's lowering AND across every
/// `js_object_assign_one` call.
///
/// The helper reads every own key of the source, so an accessor there runs
/// arbitrary user code *inside* the helper — the route #7198 accepted, having
/// declined "a helper's own allocation initiates a moving collection" on
/// evidence. `Expr::Object` has rooted its accumulator since #6951; this arm
/// never copied it.
///
/// Sabotage check: drop the `temp_root_push_double`/`temp_root_set_double` pair
/// in the `Expr::ObjectAssign` arm and the accumulator operand stops being a
/// `js_gc_temp_root_get` result.
#[test]
fn the_object_assign_accumulator_is_rooted_across_each_source() {
    let module = module_with_init(
        "object_assign_acc.ts",
        vec![Stmt::Expr(Expr::ObjectAssign {
            target: Box::new(Expr::Object(Vec::new())),
            // Two sources so the accumulator is provably live across a second
            // lowering as well as across the first helper call.
            sources: vec![Expr::Object(Vec::new()), Expr::Object(Vec::new())],
        })],
    );
    let ir = String::from_utf8(compile_module(&module, entry_opts()).unwrap())
        .expect("LLVM IR should be UTF-8");
    let f = init_ir(&ir);

    let calls: Vec<&str> = f
        .lines()
        .filter(|l| l.contains("@js_object_assign_one("))
        .collect();
    assert_eq!(
        calls.len(),
        2,
        "expected one js_object_assign_one per source in:\n{f}"
    );

    for call in &calls {
        // `call double @js_object_assign_one(double %acc, double %src)` — the
        // first operand must be re-derived from the temp root, not carried in
        // a register across the previous link.
        let acc = call
            .split_once("@js_object_assign_one(")
            .expect("argument list")
            .1
            .split(", ")
            .next()
            .and_then(|a| a.trim().strip_prefix("double "))
            .unwrap_or_else(|| panic!("no accumulator operand in `{call}`"))
            .to_string();
        assert!(
            derives_from_slot_load(f, &acc, 4),
            "the Object.assign accumulator passed to `{call}` must be re-read \
             from its root below the previous source's lowering, not carried \
             in a register (#7200). Slot traffic: {:#?}\n{f}",
            slot_traffic(f)
        );
    }

    // And the result of each link must be republished, because the helper
    // returns the target's POST-collection address. That is a SECOND store into
    // the same slot — the pooled spelling of `js_gc_temp_root_set`.
    let republished = temp_root_slots(f).into_iter().any(|slot| {
        slot_traffic(f)[&slot]
            .iter()
            .filter(|e| matches!(e, SlotEvent::Store { .. }))
            .count()
            >= 2
    });
    assert!(
        republished,
        "each js_object_assign_one result must be written back into the \
         accumulator's root — the helper returns the moved target (#7200). \
         Slot traffic: {:#?}\n{f}",
        slot_traffic(f)
    );
}

/// #7201: a `PutValueSet` KEY must be re-derived below the value's evaluation.
///
/// The dynamic-key write IC lowers `k → v → t`. That ordering is what keeps the
/// TARGET safe — the pointer is materialized below everything that can collect
/// — and the comment that used to sit there extended the claim to the key, on
/// the grounds that "a moved key merely misses by stale bits (identity compare
/// — false negatives only)". That is true of the three way-compares and false
/// of everything below them: the miss falls through to `put.dynic.slow`, which
/// hands the same register to `js_put_value_set_dyn_ic`, which DEREFERENCES it
/// as a `StringHeader*`.
///
/// A string-literal key is a load of a `__perry_init_strings_*` handle global —
/// a registered root that evacuation REWRITES — so the register loaded above
/// the value names from-space. `this.viaBlock = churn()` inside a
/// `static { … }` block is the shipped shape.
///
/// Sabotage check: remove the `guard_store_operand`/`reread_store_operand` pair
/// from the `dyn_inline` arm of `Expr::PutValueSet` and the handle load stops
/// being re-emitted below the call.
#[test]
fn a_put_value_set_key_is_re_derived_below_the_value() {
    let module = module_with_init(
        "put_value_set_key.ts",
        vec![
            Stmt::Let {
                id: 0,
                name: "o".to_string(),
                ty: perry_hir::types::Type::Any,
                init: Some(Expr::Object(Vec::new())),
                mutable: true,
            },
            Stmt::Expr(Expr::PutValueSet {
                target: Box::new(Expr::LocalGet(0)),
                key: Box::new(Expr::String("viaBlock".to_string())),
                // A call: the collection point. `Expr::Object` allocates, and
                // an allocation is a collection point in this model.
                value: Box::new(Expr::Object(Vec::new())),
                receiver: Box::new(Expr::LocalGet(0)),
                strict: true,
            }),
        ],
    );
    let ir = String::from_utf8(compile_module(&module, entry_opts()).unwrap())
        .expect("LLVM IR should be UTF-8");
    let f = init_ir(&ir);

    let lines: Vec<&str> = f.lines().collect();
    let handle_load_idxs: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("= load double, ptr @") && l.contains(".handle"))
        .map(|(i, _)| i)
        .collect();
    assert!(
        !handle_load_idxs.is_empty(),
        "expected a string-literal handle load for the key in:\n{f}"
    );
    // The key literal's handle must be loaded AFTER the value's allocation.
    // Before the fix there was exactly one such load and it sat above it.
    let alloc_idx = lines
        .iter()
        .position(|l| l.contains("= call i64 @js_object_alloc(i32 0, i32 0)"))
        .and_then(|first| {
            // the value's allocation is the SECOND `js_object_alloc` (the first
            // is the receiver `o`)
            lines
                .iter()
                .enumerate()
                .skip(first + 1)
                .find(|(_, l)| l.contains("= call i64 @js_object_alloc(i32 0, i32 0)"))
                .map(|(i, _)| i)
        })
        .unwrap_or_else(|| panic!("no value allocation in:\n{f}"));
    assert!(
        handle_load_idxs.iter().any(|i| *i > alloc_idx),
        "the key literal's handle global must be re-loaded BELOW the value's \
         allocation — a register loaded above it names from-space after an \
         evacuating minor, and the dyn-IC slow path dereferences it as a \
         StringHeader* (#7201/#7114). Handle loads at {handle_load_idxs:?}, \
         value allocation at {alloc_idx}:\n{f}"
    );
}

/// The negative half of #7202: a class that runs NO user code during
/// construction must emit no `this`-slot root at all.
///
/// The bind is gated on the same `construction_runs_user_code` predicate that
/// decides the instance needs a temp root — one predicate, one place. A class
/// with no constructor, no fields and no heritage has nothing in the window
/// that can collect, so the slot cannot go stale and the frame must not grow
/// for it.
///
/// Sabotage check: drop the `instance_root.is_some()` guard around the bind in
/// `lower_new_impl_inner` and this fails.
#[test]
fn a_collection_free_construction_emits_no_this_slot_root() {
    let _pin = NativeRootsPin::shadow();
    // `module_with_new` is the bare `Pair` class: no fields, no ctor, no
    // heritage — the exact shape `construction_runs_user_code` answers `false`
    // for.
    let ir = ir_for_new("new_inst_inert.ts", Vec::new());
    let f = init_ir(&ir);
    assert!(
        f.contains("@js_object_alloc"),
        "the fixture must actually construct something:\n{f}"
    );
    assert_no_temp_rooting(
        f,
        "#7192: an inert construction runs no user code, so it must emit no \
         instance temp root — the `this`-slot bind is gated on the same \
         predicate",
    );
    assert_eq!(
        f.matches("@js_shadow_slot_bind").count(),
        1,
        "an inert construction needs only #7876's function-lifetime class-key \
         root; it must not grow the shadow frame for a `this` slot that cannot \
         go stale (#7202):\n{f}"
    );
}
