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

    assert!(
        ir.contains("call i32 @js_gc_temp_root_push"),
        "the receiver and key must be pushed onto the temp-root stack before \
         the value's lowering, which collects (#6970):\n{ir}"
    );
    assert!(
        ir.contains("call i64 @js_gc_temp_root_get"),
        "they must be RE-READ after the value is lowered — the slot is a \
         mutable root and an evacuating cycle rewrites it:\n{ir}"
    );

    let push = ir.find("call i32 @js_gc_temp_root_push").unwrap();
    let get = ir.find("call i64 @js_gc_temp_root_get").unwrap();
    let consume = ir.find("call i64 @js_map_set(").unwrap();
    let truncate = ir.find("call void @js_gc_temp_root_truncate").unwrap();
    assert!(
        push < get && get < consume && consume < truncate,
        "order must be push → re-read → consuming call → release; the release \
         comes last because js_map_set allocates while it reads the key:\n{ir}"
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

    assert!(
        !ir.contains("call i32 @js_gc_temp_root_push"),
        "nothing after the key can collect, so this must cost exactly what it \
         cost before (the `declare` line is unconditional; only a CALL counts):\n{ir}"
    );
}

// ---------------------------------------------------------------- #6971 ----

/// `s.concat(x)` threads a bare `StringHeader*` accumulator through an SSA
/// register across every argument, and each `js_string_concat` returns a NEW
/// address — so the slot has to be written back, not just re-read.
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

    assert!(
        ir.contains("call i32 @js_gc_temp_root_push"),
        "the concat accumulator must be rooted across an allocating argument \
         (#6971):\n{ir}"
    );
    assert!(
        ir.contains("call void @js_gc_temp_root_set"),
        "each js_string_concat yields a NEW address, so the accumulator must be \
         written back into its slot — otherwise the next argument's lowering \
         keeps the INPUT alive and sweeps the string under construction:\n{ir}"
    );
    assert!(
        ir.contains("call i64 @js_gc_temp_root_get"),
        "the accumulator must be re-read after the argument (and its ToString \
         coercion, which also allocates):\n{ir}"
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

    assert!(
        !ir.contains("call i32 @js_gc_temp_root_push"),
        "a numeric argument cannot collect, so the receiver needs no root:\n{ir}"
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

    assert!(
        ir.contains("call i32 @js_gc_temp_root_push"),
        "constructor arguments must be rooted (#6969):\n{ir}"
    );
    assert!(
        ir.contains("call i64 @js_gc_temp_root_get"),
        "they must be re-read after the instance allocation:\n{ir}"
    );

    // Push order: the scope marker, then one root per heap argument — and
    // argument 0's push must precede argument 1's *lowering*, not merely
    // precede the instance allocation.
    let pushes: Vec<usize> = ir
        .match_indices("call i32 @js_gc_temp_root_push")
        .map(|(i, _)| i)
        .collect();
    assert!(
        pushes.len() >= 3,
        "expected the scope marker plus a root for each of the two heap \
         arguments, got {} pushes:\n{ir}",
        pushes.len()
    );
    // Each argument is an object literal, so each lowers to its own
    // `js_object_alloc`; the SECOND one is argument 1's, i.e. the collection
    // point argument 0 has to survive.
    let arg_allocs: Vec<usize> = ir
        .match_indices("call i64 @js_object_alloc(")
        .map(|(i, _)| i)
        .collect();
    assert!(arg_allocs.len() >= 2, "both arguments allocate:\n{ir}");
    assert!(
        pushes[1] < arg_allocs[1],
        "argument 0 must be rooted BEFORE argument 1 is lowered — rooting the \
         whole list after the loop publishes an already-dangling pointer (#6969):\n{ir}"
    );

    let instance_alloc = ir
        .find("call i64 @js_object_alloc_class_inline_keys")
        .expect("the instance allocation");
    let get = ir.find("call i64 @js_gc_temp_root_get").unwrap();
    assert!(
        pushes[pushes.len() - 1] < instance_alloc && instance_alloc < get,
        "every argument must still be rooted across the instance allocation, \
         and re-read after it:\n{ir}"
    );

    let truncate = ir.find("call void @js_gc_temp_root_truncate").unwrap();
    assert!(
        get < truncate,
        "the scope cut must come after the arguments are consumed:\n{ir}"
    );
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

    assert!(
        !ir.contains("call i32 @js_gc_temp_root_push"),
        "a number and a string literal need no root — the literal is a load \
         from a module global already registered with js_gc_register_global_root:\n{ir}"
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
    // second liveness mechanism, so this must cost zero runtime calls for it.
    // (The allocating operand still gets a real root — hence >= 1 push.)
    let pushes = ir.matches("call i32 @js_gc_temp_root_push").count();
    assert!(
        pushes <= 2,
        "expected at most the scope marker plus the allocating operand's root; \
         the literal must not get a slot of its own, got {pushes}:\n{ir}"
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
/// takes the fused `js_string_concat_value` path — the exact expression form
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

    let concat = ir
        .find("call i64 @js_string_concat_value(")
        .unwrap_or_else(|| panic!("expected the fused string+value concat:\n{ir}"));
    let alloc = ir[..concat]
        .rfind("call i64 @js_object_alloc(")
        .unwrap_or_else(|| panic!("the sibling operand must allocate:\n{ir}"));
    let handle_load = ir[..concat]
        .rfind("load double, ptr @concat_reload_ts_.str.")
        .unwrap_or_else(|| panic!("the literal must come from its handle global:\n{ir}"));

    assert!(
        handle_load > alloc,
        "#7114: the handle load that feeds the concat must sit BELOW the \
         allocating sibling. Loading it above and masking the cached register \
         below is what made `console.log(\"acc:\" + run(1e7))` print an empty \
         line — the handle global is a registered root that an evacuating \
         cycle REWRITES, and the pre-call register keeps the pre-move \
         address:\n{ir}"
    );

    assert_eq!(
        ir.matches("call i32 @js_gc_temp_root_push").count(),
        0,
        "and it must cost no runtime call. A registered root already has \
         liveness; all it was missing is the re-derivation, which is the load \
         that was going to be emitted anyway:\n{ir}"
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

    assert_eq!(
        ir.matches("load double, ptr @concat_noreload_ts_.str.")
            .count(),
        1,
        "a comparison over two immediates runs no user code and allocates \
         nothing, so the literal must be loaded exactly once:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_gc_temp_root_push"),
        "and no rooting at all:\n{ir}"
    );
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

    let handle_pat = "load double, ptr @array_reload_ts_.str.";
    let loads: Vec<usize> = ir.match_indices(handle_pat).map(|(i, _)| i).collect();
    let element_alloc = ir
        .find("call i64 @js_object_alloc(")
        .unwrap_or_else(|| panic!("element 1 must allocate:\n{ir}"));

    assert!(
        loads.iter().any(|&l| l > element_alloc),
        "#7114: element 0's handle must be re-derived below element 1's \
         allocation before it is stored into the array:\n{ir}"
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

    let push = ir
        .find("call i32 @js_gc_temp_root_push")
        .unwrap_or_else(|| panic!("a WTF-8 literal operand must be ROOTED:\n{ir}"));
    let alloc = ir
        .find("call i64 @js_object_alloc(")
        .unwrap_or_else(|| panic!("the sibling operand must allocate:\n{ir}"));
    let get = ir
        .find("call i64 @js_gc_temp_root_get")
        .unwrap_or_else(|| panic!("and re-read after it:\n{ir}"));

    assert!(
        push < alloc && alloc < get,
        "order must be push -> allocating sibling -> re-read. Anything else \
         means the WTF-8 literal is being carried across the collection point \
         in a register, which is #7114 for lone-surrogate literals:\n{ir}"
    );
}
