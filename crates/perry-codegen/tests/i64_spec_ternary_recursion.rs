//! #6221: a self-recursive function whose recursive call sits inside a
//! ternary was admitted by the i64-specialization gate (`i64s_expr` accepts
//! `Expr::Conditional`) but the i64 body emitter had no `Conditional` arm and
//! fell into its `_ => "0"` catch-all — producing an empty specialized body
//! (`ret i64 0`) that shadowed the real function. Also covers the sibling
//! gate bug: fractional `Number` literals were admitted and then truncated
//! by the emitter's `as i64` lowering.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt};

fn empty_opts() -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: false,
        non_entry_module_prefixes: Vec::new(),
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
        nextjs_path_init_modules: Vec::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}

fn number_param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Number,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

/// `function <name>(n: number): number { return n <= 0 ? <base> : <name>(n - 1); }`
fn ternary_recursive_fn(id: u32, name: &str, base: Expr) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params: vec![number_param(10, "n")],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::Conditional {
            condition: Box::new(Expr::Compare {
                op: CompareOp::Le,
                left: Box::new(Expr::LocalGet(10)),
                right: Box::new(Expr::Integer(0)),
            }),
            then_expr: Box::new(base),
            else_expr: Box::new(Expr::Call {
                callee: Box::new(Expr::FuncRef(id)),
                args: vec![Expr::Binary {
                    op: BinaryOp::Sub,
                    left: Box::new(Expr::LocalGet(10)),
                    right: Box::new(Expr::Integer(1)),
                }],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
        }))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    }
}

fn module_with(functions: Vec<Function>) -> Module {
    Module {
        name: "i64_spec_ternary.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions,
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        init: Vec::new(),
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

/// Slice out the body of the `define`d function whose name contains `marker`.
fn function_body<'a>(ir: &'a str, marker: &str) -> Option<&'a str> {
    let start = ir
        .match_indices("define ")
        .find(|(i, _)| {
            ir[*i..ir[*i..].find('\n').map(|n| i + n).unwrap_or(ir.len())].contains(marker)
        })
        .map(|(i, _)| i)?;
    let end = ir[start..].find("\n}")? + start;
    Some(&ir[start..end])
}

#[test]
fn ternary_self_recursion_gets_real_i64_body() {
    let f = ternary_recursive_fn(1, "idDown", Expr::Number(100.0));
    let ir =
        String::from_utf8(compile_module(&module_with(vec![f]), empty_opts()).unwrap()).unwrap();

    let body =
        function_body(&ir, "idDown_i64").expect("i64 specialization for idDown should be emitted");
    // The specialized body must branch on the ternary condition and make the
    // self-recursive call — not collapse to the empty `ret i64 0` stub.
    assert!(
        body.contains("br i1"),
        "ternary must lower to a conditional branch, got:\n{body}"
    );
    assert!(
        body.contains("call i64"),
        "recursive call must survive in the i64 body, got:\n{body}"
    );
    assert!(
        !body.trim_end().ends_with("ret i64 0") || body.contains("br i1"),
        "i64 body is the empty stub:\n{body}"
    );
}

#[test]
fn fractional_literal_blocks_i64_specialization() {
    // `return n <= 0 ? 0.5 : halfDown(n - 1);` — the i64 emitter would
    // truncate 0.5 to 0, so the gate must reject the function entirely and
    // leave the exact f64 body in place.
    let f = ternary_recursive_fn(1, "halfDown", Expr::Number(0.5));
    let ir =
        String::from_utf8(compile_module(&module_with(vec![f]), empty_opts()).unwrap()).unwrap();

    assert!(
        !ir.contains("halfDown_i64"),
        "function with a fractional literal must not be i64-specialized"
    );
}
