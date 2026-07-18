//! #6511 — cargo-test-visible coverage for the Math.*-result arithmetic
//! routing (the integration twin lives in
//! `tests/native_proof_regressions/math_mul_fastpath.rs` and only runs on
//! nightly/tag workflows). A multiply of `Math.*` results must stay on the
//! inline `fmul` fast path; a possibly-object operand must keep the
//! BigInt-aware `js_dynamic_mul` routing from #5970.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{
    BinaryOp, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt, UpdateOp,
};
use perry_types::Type;

fn ir_opts() -> CompileOptions {
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

fn probe_module(name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Module {
    Module {
        name: name.to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
            type_params: Vec::new(),
            params,
            return_type: Type::Number,
            body,
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }],
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

fn emitted_ir(module: Module) -> String {
    String::from_utf8(compile_module(&module, ir_opts()).unwrap()).expect("LLVM IR should be UTF-8")
}

fn number_let(id: u32, name: &str, mutable: bool, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Number,
        mutable,
        init: Some(init),
    }
}

fn mul(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Mul,
        left: Box::new(left),
        right: Box::new(right),
    }
}

#[test]
fn math_result_multiply_stays_inline_fmul() {
    // The #6511 repro's accumulator-loop shape, with a call-free MathSin
    // operand (`Math.sin(i)`, not the repro's `i * 0.001`) so the only
    // `fmul` in the function is the Math-result multiply under test:
    // `for (i = 0; i < 64; i++) acc += Math.sqrt(i) * Math.sin(i);`
    let ir = emitted_ir(probe_module(
        "math_result_multiply_unit.ts",
        Vec::new(),
        vec![
            number_let(1, "acc", true, Expr::Integer(0)),
            number_let(3, "iterations", false, Expr::Integer(64)),
            Stmt::For {
                init: Some(Box::new(number_let(2, "i", true, Expr::Integer(0)))),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::LocalGet(3)),
                }),
                update: Some(Expr::Update {
                    id: 2,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    1,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(1)),
                        right: Box::new(mul(
                            Expr::MathSqrt(Box::new(Expr::LocalGet(2))),
                            Expr::MathSin(Box::new(Expr::LocalGet(2))),
                        )),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    ));
    assert!(
        ir.contains("call double @llvm.sqrt.f64") && ir.contains("call double @llvm.sin.f64"),
        "Math.sqrt / Math.sin should lower to their intrinsics:\n{ir}"
    );
    assert!(
        ir.contains("fmul double"),
        "a multiply of Math.* results must emit an inline fmul:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_dynamic_mul"),
        "a multiply of Math.* results must not route through the boxed \
         BigInt-aware multiply helper:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_number_coerce"),
        "Math.* results are already raw doubles — the fast path must not \
         re-coerce them:\n{ir}"
    );
}

#[test]
fn dynamic_operand_multiply_keeps_bigint_aware_helper() {
    // #5970's correctness routing must survive: an operand that may be an
    // object (possible boxed BigInt / BigInt-returning valueOf) still goes
    // through the ToNumeric-running dynamic helper.
    let ir = emitted_ir(probe_module(
        "math_dynamic_operand_multiply_unit.ts",
        vec![Param {
            id: 2,
            name: "x".to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        vec![Stmt::Return(Some(mul(
            Expr::MathSqrt(Box::new(Expr::Integer(4))),
            Expr::LocalGet(2),
        )))],
    ));
    assert!(
        ir.contains("call double @js_dynamic_mul"),
        "a possibly-object operand must keep the BigInt-aware dynamic \
         multiply routing:\n{ir}"
    );
}
