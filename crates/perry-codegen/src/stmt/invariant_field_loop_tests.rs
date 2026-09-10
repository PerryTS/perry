//! Admission and non-observability contracts for invariant own-number reads.
use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Module, Stmt, UpdateOp};

fn ir_opts() -> CompileOptions {
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

fn ir(index: Expr, extra: Option<Stmt>, bound: Expr) -> String {
    let mut m = Module::new("invariant_field_loop.ts");
    let mut body = vec![Stmt::Expr(Expr::LocalSet(
        3,
        Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(3)),
            right: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(index),
                }),
                property: "id".into(),
                byte_offset: 0,
            }),
        }),
    ))];
    if let Some(extra) = extra {
        body.push(extra);
    }
    m.functions = vec![perry_hir::Function {
        id: 900,
        name: "total".into(),
        type_params: Vec::new(),
        params: [(1, "rows", Type::Any), (2, "count", Type::Number)]
            .into_iter()
            .map(|(id, name, ty)| perry_hir::Param {
                id,
                name: name.into(),
                ty,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            })
            .collect(),
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: 3,
                name: "sum".into(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(0.0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 4,
                    name: "i".into(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(4)),
                    right: Box::new(bound),
                }),
                update: Some(Expr::Update {
                    id: 4,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body,
            },
            Stmt::Return(Some(Expr::LocalGet(3))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    String::from_utf8(compile_module(&m, ir_opts()).unwrap()).unwrap()
}

#[test]
fn invariant_field_loop_is_entered_and_arithmetic_clone_is_call_free() {
    let ir = ir(Expr::Integer(7), None, Expr::LocalGet(2));
    let probes = ir
        .matches("call double @js_array_index_own_number(")
        .count();
    assert!(probes > 0, "{ir}");
    assert!(ir.contains("label %invariant_field.fast.preheader"));
    let mut fast = false;
    let mut fast_blocks = 0;
    let mut additions = 0;
    for line in ir.lines() {
        if !line.starts_with(' ') && line.contains(':') {
            fast = line.starts_with("invariant_field.fast.");
            if fast {
                fast_blocks += 1;
            }
        }
        if fast {
            // RS4GC's tied empty asm emits no instruction and transfers no
            // control (same census contract as element_shape_loop_tests).
            let root_reload = line.contains("asm \"\", \"=r,0\"");
            assert!(
                (!line.contains("call ") || root_reload) && !line.contains("invoke "),
                "{line}"
            );
            assert!(
                !line.contains("reassoc"),
                "must preserve sequential addition"
            );
            if line.contains("fadd double") {
                additions += 1;
            }
        }
    }
    // Generic and argument-specialized function variants each get one probe.
    assert_eq!(fast_blocks, 3 * probes);
    assert_eq!(additions, probes);
    assert!(
        ir.contains("fcmp ogt double"),
        "positive trip guard before probe"
    );
    assert!(
        ir.contains("for.invariant_field_slow"),
        "ordinary semantics remain reachable"
    );
    assert_eq!(
        crate::gc_call_effects::classify_direct_callee("js_array_index_own_number"),
        crate::gc_call_effects::GcCallEffect::CannotCollect
    );
}

#[test]
fn invariant_field_loop_rejects_changing_indices_and_effectful_bodies() {
    for index in [
        Expr::LocalGet(4),
        Expr::Number(0.5),
        Expr::Integer(-1),
        Expr::Integer(u32::MAX as i64),
    ] {
        assert!(
            !ir(index, None, Expr::LocalGet(2)).contains("call double @js_array_index_own_number(")
        );
    }
    let mutation = Stmt::Expr(Expr::LocalSet(2, Box::new(Expr::Number(0.0))));
    assert!(!ir(Expr::Integer(7), Some(mutation), Expr::LocalGet(2))
        .contains("call double @js_array_index_own_number("));
    // An accumulator bound changes each iteration and cannot be hoisted.
    assert!(!ir(Expr::Integer(7), None, Expr::LocalGet(3))
        .contains("call double @js_array_index_own_number("));
}
