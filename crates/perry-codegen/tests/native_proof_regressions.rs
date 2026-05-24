use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Module, ModuleInitKind, Stmt, UpdateOp};
use perry_types::Type;

fn empty_opts() -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: false,
        non_entry_module_prefixes: Vec::new(),
        import_function_prefixes: std::collections::HashMap::new(),
        import_function_origin_names: std::collections::HashMap::new(),
        import_function_v8_specifiers: std::collections::HashMap::new(),
        import_function_node_submodule: std::collections::HashMap::new(),
        namespace_node_submodules: std::collections::HashMap::new(),
        namespace_v8_specifiers: std::collections::HashMap::new(),
        namespace_member_prefixes: std::collections::HashMap::new(),
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
    }
}

fn module(name: &str, body: Vec<Stmt>) -> Module {
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
            params: Vec::new(),
            return_type: Type::Number,
            body,
            is_async: false,
            is_generator: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }],
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
    }
}

fn compile_ir(name: &str, body: Vec<Stmt>) -> String {
    String::from_utf8(compile_module(&module(name, body), empty_opts()).unwrap()).unwrap()
}

fn local(id: u32) -> Expr {
    Expr::LocalGet(id)
}

fn int(value: i64) -> Expr {
    Expr::Integer(value)
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

fn buffer_let(id: u32, name: &str, size: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Named("Buffer".to_string()),
        mutable: false,
        init: Some(Expr::BufferAlloc {
            size: Box::new(size),
            fill: None,
            encoding: None,
        }),
    }
}

fn number_array_let(id: u32, name: &str, values: Vec<i64>) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Array(Box::new(Type::Number)),
        mutable: true,
        init: Some(Expr::Array(values.into_iter().map(int).collect())),
    }
}

fn bit_or_zero(value: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::BitOr,
        left: Box::new(value),
        right: Box::new(int(0)),
    }
}

fn div(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Div,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn add(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn length(local_id: u32) -> Expr {
    Expr::PropertyGet {
        object: Box::new(local(local_id)),
        property: "length".to_string(),
    }
}

fn buffer_set(buffer_id: u32, index: Expr) -> Stmt {
    Stmt::Expr(Expr::BufferIndexSet {
        buffer: Box::new(local(buffer_id)),
        index: Box::new(index),
        value: Box::new(int(1)),
    })
}

fn array_set(array_id: u32, index: Expr, value: Expr) -> Stmt {
    Stmt::Expr(Expr::IndexSet {
        object: Box::new(local(array_id)),
        index: Box::new(index),
        value: Box::new(value),
    })
}

fn increment(id: u32) -> Expr {
    Expr::Update {
        id,
        op: UpdateOp::Increment,
        prefix: false,
    }
}

fn decrement(id: u32) -> Expr {
    Expr::Update {
        id,
        op: UpdateOp::Decrement,
        prefix: false,
    }
}

fn for_loop_with_start_and_update(
    counter_id: u32,
    start: Expr,
    bound: Expr,
    update: Option<Expr>,
    body: Vec<Stmt>,
) -> Stmt {
    for_loop_with_op_start_and_update(counter_id, start, CompareOp::Lt, bound, update, body)
}

fn for_loop_with_op_start_and_update(
    counter_id: u32,
    start: Expr,
    op: CompareOp,
    bound: Expr,
    update: Option<Expr>,
    body: Vec<Stmt>,
) -> Stmt {
    Stmt::For {
        init: Some(Box::new(number_let(counter_id, "i", true, start))),
        condition: Some(Expr::Compare {
            op,
            left: Box::new(local(counter_id)),
            right: Box::new(bound),
        }),
        update,
        body,
    }
}

fn for_loop(counter_id: u32, bound: Expr, body: Vec<Stmt>) -> Stmt {
    for_loop_with_start_and_update(counter_id, int(0), bound, Some(increment(counter_id)), body)
}

fn assert_buffer_store_uses_dynamic_fallback(ir: &str) {
    assert!(
        ir.contains("call void @js_buffer_set"),
        "stale-proof case should keep the checked Buffer store fallback:\n{ir}"
    );
    assert!(
        !ir.contains("getelementptr inbounds i8"),
        "stale-proof case must not emit an inbounds native buffer GEP:\n{ir}"
    );
}

fn block_between<'a>(ir: &'a str, start: &str, end: &str) -> &'a str {
    let start_pos = ir.find(start).unwrap_or_else(|| {
        panic!("missing block start marker {start:?} in IR:\n{ir}");
    });
    let after_start = &ir[start_pos + 1..];
    let end_pos = after_start.find(end).unwrap_or_else(|| {
        panic!("missing block end marker {end:?} after {start:?} in IR:\n{ir}");
    });
    &after_start[..end_pos]
}

#[test]
fn localset_invalidates_native_i32_alias_facts() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop(
            2,
            length(1),
            vec![
                number_let(3, "j", true, bit_or_zero(local(2))),
                Stmt::Expr(Expr::LocalSet(3, Box::new(int(16)))),
                buffer_set(1, local(3)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("native_i32_alias_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn update_invalidates_native_i32_alias_facts() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop(
            2,
            length(1),
            vec![
                number_let(3, "j", true, bit_or_zero(local(2))),
                Stmt::Expr(increment(3)),
                buffer_set(1, local(3)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("native_i32_alias_update_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn localset_invalidates_min_length_facts() {
    let body = vec![
        buffer_let(1, "src", int(8)),
        buffer_let(2, "dst", int(8)),
        number_let(3, "n", true, Expr::MathMin(vec![length(1), length(2)])),
        Stmt::Expr(Expr::LocalSet(3, Box::new(int(16)))),
        for_loop(4, local(3), vec![buffer_set(2, local(4))]),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("min_length_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn localset_invalidates_active_bounded_buffer_index_facts() {
    let body = vec![
        number_let(1, "n", false, int(8)),
        buffer_let(2, "buf", local(1)),
        for_loop(
            3,
            local(1),
            vec![
                Stmt::Expr(Expr::LocalSet(3, Box::new(int(16)))),
                buffer_set(2, local(3)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("bounded_buffer_index_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn inner_loop_bounded_buffer_fact_is_removed_after_outer_fact_invalidation() {
    let body = vec![
        number_let(1, "n", false, int(8)),
        buffer_let(2, "a", local(1)),
        buffer_let(3, "b", int(8)),
        for_loop(
            4,
            local(1),
            vec![
                for_loop(
                    5,
                    length(3),
                    vec![Stmt::Expr(Expr::LocalSet(4, Box::new(int(16))))],
                ),
                buffer_set(3, local(5)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("nested_loop_scope_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn localset_invalidates_buffer_view_local_length_sources() {
    let body = vec![
        number_let(1, "n", true, int(8)),
        buffer_let(2, "buf", local(1)),
        Stmt::Expr(Expr::LocalSet(1, Box::new(int(16)))),
        for_loop(3, local(1), vec![buffer_set(2, local(3))]),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("buffer_length_source_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn update_invalidates_buffer_view_local_length_sources() {
    let body = vec![
        number_let(1, "n", true, int(8)),
        buffer_let(2, "buf", local(1)),
        Stmt::Expr(increment(1)),
        for_loop(3, local(1), vec![buffer_set(2, local(3))]),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("buffer_length_source_update_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn negative_loop_counter_does_not_emit_inbounds_buffer_gep() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop_with_start_and_update(
            2,
            int(-1),
            length(1),
            Some(increment(2)),
            vec![buffer_set(1, local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("negative_loop_counter_buffer_bounds.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn decrementing_loop_update_does_not_emit_inbounds_buffer_gep() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop_with_start_and_update(
            2,
            int(0),
            length(1),
            Some(decrement(2)),
            vec![buffer_set(1, local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("decrementing_loop_update_buffer_bounds.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn body_counter_mutation_does_not_emit_inbounds_buffer_gep() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop(
            2,
            length(1),
            vec![Stmt::Expr(decrement(2)), buffer_set(1, local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("body_counter_mutation_buffer_bounds.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn inclusive_length_loop_does_not_emit_inbounds_buffer_gep() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop_with_op_start_and_update(
            2,
            int(0),
            CompareOp::Le,
            length(1),
            Some(increment(2)),
            vec![buffer_set(1, local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("inclusive_length_loop_buffer_bounds.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
    let cond_ir = block_between(&ir, "\nfor.cond.", "\nfor.body.");
    assert!(
        cond_ir.contains("icmp sle i32"),
        "`i <= buf.length` with hoisted i32 length must lower as signed <=:\n{cond_ir}"
    );
    assert!(
        !cond_ir.contains("icmp slt i32"),
        "`i <= buf.length` must not be narrowed to signed <:\n{cond_ir}"
    );
}

#[test]
fn inclusive_array_length_write_uses_extension_capable_index_set_path() {
    let body = vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        for_loop_with_op_start_and_update(
            2,
            int(0),
            CompareOp::Le,
            length(1),
            Some(increment(2)),
            vec![array_set(1, local(2), local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("inclusive_array_length_write.ts", body);
    assert!(
        ir.contains("\nidxset.check_cap."),
        "`arr[i]` under `i <= arr.length` must keep the capacity check path:\n{ir}"
    );
    assert!(
        ir.contains("\nidxset.extend_inline."),
        "`arr[i]` under `i <= arr.length` must keep the inline length-extension path:\n{ir}"
    );
    assert!(
        ir.contains("call i64 @js_array_set_f64_extend"),
        "`arr[i]` under `i <= arr.length` must keep the realloc-capable fallback:\n{ir}"
    );
}

#[test]
fn inclusive_local_length_bound_does_not_use_local_length_bound_fact() {
    let body = vec![
        number_let(1, "n", false, int(8)),
        buffer_let(2, "buf", local(1)),
        for_loop_with_op_start_and_update(
            3,
            int(0),
            CompareOp::Le,
            local(1),
            Some(increment(3)),
            vec![buffer_set(2, local(3))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("inclusive_local_length_bound.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn negative_loop_counter_does_not_use_local_length_bound_fact() {
    let body = vec![
        number_let(1, "n", false, int(8)),
        buffer_let(2, "buf", local(1)),
        for_loop_with_start_and_update(
            3,
            int(-1),
            local(1),
            Some(increment(3)),
            vec![buffer_set(2, local(3))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("negative_counter_local_length_bound.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn negative_loop_counter_does_not_use_min_length_bound_fact() {
    let body = vec![
        buffer_let(1, "src", int(8)),
        buffer_let(2, "dst", int(8)),
        number_let(3, "n", false, Expr::MathMin(vec![length(1), length(2)])),
        for_loop_with_start_and_update(
            4,
            int(-1),
            local(3),
            Some(increment(4)),
            vec![buffer_set(2, local(4))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("negative_counter_min_length_bound.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn bitwise_truncated_division_does_not_emit_sdiv_i32() {
    let quotient = bit_or_zero(div(local(1), local(2)));
    let divide_by_zero = bit_or_zero(div(local(1), int(0)));
    let overflow = bit_or_zero(div(int(i32::MIN as i64), int(-1)));
    let body = vec![
        number_let(1, "x", false, int(8)),
        number_let(2, "y", false, int(2)),
        Stmt::Return(Some(add(add(quotient, divide_by_zero), overflow))),
    ];

    let ir = compile_ir("i32_division_regression.ts", body);
    assert!(
        !ir.contains("sdiv i32"),
        "`(a / b) | 0` must not lower to LLVM signed integer division:\n{ir}"
    );
    assert!(
        ir.contains("fdiv double"),
        "`(a / b) | 0` should lower through JS double division:\n{ir}"
    );
    assert!(
        ir.contains("@llvm.fabs.f64"),
        "ToInt32 after division should keep the NaN/Infinity guard:\n{ir}"
    );
}
