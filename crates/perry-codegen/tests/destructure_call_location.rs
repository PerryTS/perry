//! Regression test for #5247 (coverage gap): object destructuring of a
//! `null`/`undefined` source throws "Cannot convert undefined or null to
//! object" via the `requireObjectCoercible` runtime call. Before this fix the
//! throw rendered the *stale* last-tracked call location (often in an unrelated
//! module) because the destructuring path emitted no `js_set_call_location`.
//!
//! The HIR destructuring lowering now carries the object-pattern's source byte
//! offset as a second literal argument to the `requireObjectCoercible` call;
//! under `--debug-symbols` codegen emits a `js_set_call_location(file, line)`
//! immediately before `js_require_object_coercible` so the thrown TypeError's
//! `.stack` points at the destructure.
//!
//! This asserts the codegen contract directly on the emitted LLVM IR: with a
//! debug-location context installed, the location call precedes the coercibility
//! check; with no `--debug-symbols` context, no location call is emitted (the
//! default build stays byte-identical / overhead-free).

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Expr, Module, ModuleInitKind, Stmt};

fn base_opts() -> CompileOptions {
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
        namespace_reexport_named_imports: std::collections::HashSet::new(),
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

/// A module whose init evaluates `RequireObjectCoercible(undefined)` exactly as
/// the destructuring lowering emits it: a `__perry_runtime` NativeMethodCall
/// with the source `undefined` plus the object-pattern byte offset as a second
/// `Number` arg. Offset 8 sits on line 2 of the source below.
fn module_with_destructure_coercible() -> Module {
    let mut m = Module::new("destr.ts");
    m.init = vec![Stmt::Expr(Expr::NativeMethodCall {
        module: "__perry_runtime".to_string(),
        class_name: None,
        object: None,
        method: "requireObjectCoercible".to_string(),
        // arg 0 = source value; arg 1 = object-pattern byte offset (1-based).
        // BytePos 8 → source index 7 ('a' on line 2) → line 2.
        args: vec![Expr::Undefined, Expr::Number(8.0)],
    })];
    m.init_kind = ModuleInitKind::Eager;
    m
}

#[test]
fn destructure_emits_call_location_before_coercible_check() {
    let mut opts = base_opts();
    opts.debug_locations = true;
    // "x();\nconst {a} = o;\n" — BytePos 8 lands on line 2.
    opts.module_source = Some("x();\nconst {a} = o;\n".to_string());

    let ir = String::from_utf8(compile_module(&module_with_destructure_coercible(), opts).unwrap())
        .expect("LLVM IR should be UTF-8");

    // Match the CALLS, not the always-present `declare`s in the runtime
    // preamble (which appear earlier in the IR than any call).
    let set_loc = ir
        .find("call void @js_set_call_location")
        .expect("expected a js_set_call_location call under --debug-symbols:\n");
    let coercible = ir
        .find("call double @js_require_object_coercible")
        .expect("expected a js_require_object_coercible call in IR");
    assert!(
        set_loc < coercible,
        "js_set_call_location must precede js_require_object_coercible:\n{ir}"
    );
}

#[test]
fn no_call_location_without_debug_symbols() {
    // Default build: debug_locations off → no per-destructure location call,
    // and the coercibility check is still emitted (behavior unchanged).
    let ir = String::from_utf8(
        compile_module(&module_with_destructure_coercible(), base_opts()).unwrap(),
    )
    .expect("LLVM IR should be UTF-8");
    assert!(
        ir.contains("call double @js_require_object_coercible"),
        "coercibility check must still be emitted by default:\n{ir}"
    );
    // `js_set_call_location` is always `declare`d in the runtime preamble; the
    // contract is that no CALL to it is emitted in the default build.
    assert!(
        !ir.contains("call void @js_set_call_location"),
        "no js_set_call_location CALL should be emitted without --debug-symbols:\n{ir}"
    );
}
