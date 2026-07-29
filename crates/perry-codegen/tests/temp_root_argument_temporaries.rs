//! #6951: evaluated-but-not-yet-consumed argument temporaries must be precise
//! GC roots, not bare LLVM SSA registers.
//!
//! The end-to-end proof lives in the GC × representation matrix
//! (`scripts/gc_repsel_matrix.sh`, `cons_scan_off` arm), which runs the corpus
//! with `PERRY_CONSERVATIVE_STACK_SCAN=off` — the only configuration where the
//! bug is observable, because every automatic collection otherwise forces a
//! conservative native-stack scan that pins the temporary by accident. These
//! tests pin the *codegen contract* that arm depends on, in-process and in
//! `cargo-test`, so a lowering path that quietly goes back to threading an
//! accumulator through an SSA register fails here rather than three weeks
//! later under a narrowed forced scan.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Expr, Module, ModuleInitKind, Stmt};

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

/// `console.log(a, b, …)` — the shape in the #6951 repro.
fn console_log(args: Vec<Expr>) -> Stmt {
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
fn allocating() -> Expr {
    Expr::Object(Vec::new())
}

/// The argument accumulator of a variadic call must live in a temp root, and
/// every use of it must be re-read from that root.
///
/// Pre-fix the sequence was `js_array_alloc` → N × `js_array_push_f64` with the
/// accumulator threaded through an SSA register across each argument's
/// evaluation. That register held the ONLY reference to everything pushed so
/// far, so an allocating later argument swept the half-built array and the
/// next push landed in recycled memory — `console.log("label", churn())` lost
/// its label with no crash and no diagnostic.
#[test]
fn console_argument_accumulator_is_temp_rooted() {
    let ir = ir_for(
        "console_accumulator.ts",
        vec![console_log(vec![
            Expr::String("label".to_string()),
            allocating(),
        ])],
    );

    assert!(
        ir.contains("call i32 @js_gc_temp_root_push"),
        "the accumulator must be pushed onto the temp-root stack:\n{ir}"
    );
    assert!(
        ir.contains("call void @js_array_push_f64_temp_rooted"),
        "arguments must be appended through the rooted push, which reads the \
         accumulator out of its slot and writes the reallocated pointer back:\n{ir}"
    );
    assert!(
        ir.contains("call i64 @js_gc_temp_root_get"),
        "the accumulator must be RE-READ before the consuming call — an \
         evacuating cycle rewrites the slot, so the pushed register is stale:\n{ir}"
    );
    assert!(
        ir.contains("call void @js_gc_temp_root_truncate"),
        "the temp root must be released after the consuming call:\n{ir}"
    );

    // The accumulator must not survive as a threaded SSA register: the raw
    // two-operand push is what made it unrooted in the first place.
    assert!(
        !ir.contains("call i64 @js_array_push_f64(i64"),
        "the console argument list must not thread the accumulator through an \
         SSA register any more (#6951):\n{ir}"
    );

    // Ordering: read, consume, then release.
    let get = ir.find("call i64 @js_gc_temp_root_get").unwrap();
    let consume = ir.find("call void @js_console_log_spread").unwrap();
    let truncate = ir.find("call void @js_gc_temp_root_truncate").unwrap();
    assert!(
        get < consume && consume < truncate,
        "the accumulator must be read before the consumer runs and released \
         only after it returns:\n{ir}"
    );
}

/// The gate: rooting is emitted only when something that follows can collect.
///
/// An array literal of plain string literals has no allocation between its
/// elements' evaluation and the array's construction, so it must emit exactly
/// the IR it emitted before #6951 — no runtime calls, no cost.
#[test]
fn non_allocating_element_list_emits_no_rooting_calls() {
    let ir = ir_for(
        "array_literal_no_gc.ts",
        vec![Stmt::Expr(Expr::Array(vec![
            Expr::String("a".to_string()),
            Expr::String("b".to_string()),
            Expr::Number(1.0),
        ]))],
    );

    assert!(
        !ir.contains("call i32 @js_gc_temp_root_push"),
        "an all-literal array literal must not pay for temp rooting (the \
         `declare` line is unconditional; only an emitted CALL counts):\n{ir}"
    );
}

/// …and it IS emitted when a later element allocates: the earlier element's
/// value is in an SSA register across that allocation, which is not a root.
#[test]
fn array_literal_roots_elements_before_an_allocating_element() {
    let ir = ir_for(
        "array_literal_gc.ts",
        vec![Stmt::Expr(Expr::Array(vec![
            Expr::String("a".to_string()),
            allocating(),
        ]))],
    );

    assert!(
        ir.contains("call i32 @js_gc_temp_root_push"),
        "an array literal with an allocating later element must root the \
         elements already evaluated (#6951):\n{ir}"
    );
}
