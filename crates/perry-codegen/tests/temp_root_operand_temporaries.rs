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

/// The gate: `new Pair(a, b)` on plain locals must emit no rooting.
///
/// A `LocalGet` is already a precise root — codegen binds every pointer-typed
/// local to a shadow-stack slot — so paying for a temp root there would be a
/// cost on a shape that was never broken.
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
