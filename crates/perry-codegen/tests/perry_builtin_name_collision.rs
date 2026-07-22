//! Issue #6087 — the name-keyed `perry/system` | `perry/updater` |
//! `perry/background` dispatch tables must not hijack a call to a user
//! function that merely *shares a name* with one of their rows.
//!
//! A function imported from a plain TypeScript module lowers to
//! `Expr::ExternFuncRef { name }` — the very same shape a `perry/system`
//! import produces. `lower_call/extern_func.rs` used to consult the three
//! tables on the bare name, so `import { takeScreenshot } from "./lib.ts"`
//! was routed into `perry_system_take_screenshot`. Because the user's arity
//! (1) doesn't match that row's (0), `lower_perry_ui_table_call` then lowered
//! the args for side effects and returned the 0.0 sentinel — the call
//! **vanished**, silently, with no error and no warning.
//!
//! The lookups are now gated on the callee's import source. These tests pin
//! both directions: a `./lib.ts` import must reach the cross-module symbol,
//! and a genuine `perry/system` import must still reach the native one.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Import, ImportSpecifier, Module, ModuleInitKind, ModuleKind, Stmt};

fn base_opts() -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: false,
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

fn import_from(source: &str, name: &str, kind: ModuleKind) -> Import {
    Import {
        source: source.to_string(),
        specifiers: vec![ImportSpecifier::Named {
            imported: name.to_string(),
            local: name.to_string(),
        }],
        is_native: !source.starts_with("./"),
        module_kind: kind,
        resolved_path: Some(source.to_string()),
        type_only: false,
        is_dynamic: false,
        is_dynamic_target: false,
        is_deferred_require: false,
        is_adopted_require: false,
    }
}

fn call_extern(name: &str, args: Vec<Expr>) -> Stmt {
    Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: name.to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    })
}

fn module_with(imports: Vec<Import>, init: Vec<Stmt>) -> Module {
    Module {
        name: "app.ts".to_string(),
        imports,
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

/// `import { takeScreenshot } from "./lib.ts"; takeScreenshot("a.png")` must
/// call the user's function, NOT `perry_system_take_screenshot`. Pre-fix the
/// arity mismatch (1 arg vs the table row's 0) made the call disappear
/// entirely — the emitted IR contained neither symbol.
#[test]
fn user_import_named_like_perry_system_row_is_not_hijacked() {
    let mut opts = base_opts();
    opts.import_function_prefixes
        .insert("takeScreenshot".to_string(), "lib_ts".to_string());

    let module = module_with(
        vec![import_from(
            "./lib.ts",
            "takeScreenshot",
            ModuleKind::NativeCompiled,
        )],
        vec![call_extern(
            "takeScreenshot",
            vec![Expr::String("a.png".to_string())],
        )],
    );

    let ir = String::from_utf8(compile_module(&module, opts).expect("must compile")).unwrap();
    assert!(
        ir.contains("perry_fn_lib_ts__takeScreenshot"),
        "imported user function must lower to its cross-module symbol; IR:\n{ir}"
    );
    assert!(
        !ir.contains("perry_system_take_screenshot"),
        "a `./lib.ts` import must never be hijacked by PERRY_SYSTEM_TABLE; IR:\n{ir}"
    );
}

/// Same shape for the `perry/background` table: `cancel` is one of its rows,
/// and it is a wildly plausible user function name. Here the arity even
/// *matches* the native row, which pre-fix produced a link error against an
/// undefined `perry_background_cancel` in a program importing nothing native.
#[test]
fn user_import_named_like_perry_background_row_is_not_hijacked() {
    let mut opts = base_opts();
    opts.import_function_prefixes
        .insert("cancel".to_string(), "jobs_ts".to_string());

    let module = module_with(
        vec![import_from(
            "./jobs.ts",
            "cancel",
            ModuleKind::NativeCompiled,
        )],
        vec![call_extern(
            "cancel",
            vec![Expr::String("job-1".to_string())],
        )],
    );

    let ir = String::from_utf8(compile_module(&module, opts).expect("must compile")).unwrap();
    assert!(
        ir.contains("perry_fn_jobs_ts__cancel"),
        "imported user function must lower to its cross-module symbol; IR:\n{ir}"
    );
    assert!(
        !ir.contains("perry_background_cancel"),
        "a `./jobs.ts` import must never be hijacked by PERRY_BACKGROUND_TABLE; IR:\n{ir}"
    );
}

/// The legitimate path is untouched: a real `perry/system` import still
/// dispatches to the native runtime symbol.
#[test]
fn perry_system_import_still_dispatches_to_native_symbol() {
    let module = module_with(
        vec![import_from(
            "perry/system",
            "takeScreenshot",
            ModuleKind::NativeRust,
        )],
        vec![call_extern("takeScreenshot", vec![])],
    );

    let ir =
        String::from_utf8(compile_module(&module, base_opts()).expect("must compile")).unwrap();
    assert!(
        ir.contains("perry_system_take_screenshot"),
        "genuine perry/system import must still reach the native symbol; IR:\n{ir}"
    );
}

/// A `perry/system` builtin whose declared runtime ABI can't accept the given
/// arity is now a hard compile error instead of a call that silently vanishes.
/// (`isDarkMode` takes 0 args; the surplus arg is not an inline-style object —
/// its return kind is F64, not Widget.)
#[test]
fn perry_system_builtin_arity_mismatch_is_a_compile_error() {
    let module = module_with(
        vec![import_from(
            "perry/system",
            "isDarkMode",
            ModuleKind::NativeRust,
        )],
        vec![call_extern(
            "isDarkMode",
            vec![Expr::String("surplus".to_string())],
        )],
    );

    let err = compile_module(&module, base_opts())
        .expect_err("arity mismatch on a perry/* builtin must not compile to a dropped call");
    // The bail is wrapped in the lowering context chain, so match the whole
    // chain (`{:?}` on an anyhow::Error prints the causes too).
    let msg = format!("{err:?}");
    assert!(
        msg.contains("isDarkMode") && msg.contains("takes 0 argument"),
        "expected an arity diagnostic naming the builtin, got: {msg}"
    );
}

/// TS-optional trailing params stay legal — and now actually emit the call.
/// `shareText(text, title?)` declares `[Str, Str]` in PERRY_SYSTEM_TABLE; a
/// one-arg call pads the title with `""` rather than being dropped.
#[test]
fn perry_system_optional_trailing_arg_pads_and_emits_the_call() {
    let module = module_with(
        vec![import_from(
            "perry/system",
            "shareText",
            ModuleKind::NativeRust,
        )],
        vec![call_extern(
            "shareText",
            vec![Expr::String("hello".to_string())],
        )],
    );

    let ir =
        String::from_utf8(compile_module(&module, base_opts()).expect("must compile")).unwrap();
    assert!(
        ir.contains("perry_system_share_text"),
        "a one-arg shareText(text) must still emit the native call; IR:\n{ir}"
    );
}
