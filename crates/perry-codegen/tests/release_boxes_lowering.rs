//! `Stmt::ReleaseBoxes` must LOWER — an emitted release that never reaches
//! the IR is exactly the defect class this exists to gate (#7933 follow-up:
//! the transform emitted releases while a missed exhaustive-match arm kept
//! the compiler from building, and a stale binary silently ran the old
//! clear-only path with `releases=0` at runtime; only the runtime counter
//! caught it). These tests pin the two halves codegen owns:
//!
//!   1. kind selection — the compiler-private `__gen_state` (i32) and
//!      `__gen_done`/`__gen_executing` (i1) control cells route to their
//!      typed release entry points, everything else to `js_box_release`;
//!   2. the capture path — a release inside the synthesized step closure
//!      (where the cells arrive as captures, not local slots) still lowers.
//!
//! Assertions match CALL SITES (`call void @js_box_release`), never the
//! `declare` lines, which are emitted unconditionally.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
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
        namespace_member_nested: Vec::new(),
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
        local_source_spans: std::collections::HashMap::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

/// The statements go in as a FUNCTION body — the real release site
/// (a module-init `let` promotes to a `@perry_global_*` slot, which a
/// release correctly skips; only function-scoped cells release).
fn ir_for_fn_body(name: &str, body: Vec<Stmt>) -> String {
    let mut m = module_with_init(name, Vec::new());
    m.functions.push(perry_hir::Function {
        id: 7000,
        name: "driver".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    String::from_utf8(compile_module(&m, entry_opts()).unwrap()).expect("LLVM IR should be UTF-8")
}

const STATE: u32 = 1;
const DONE: u32 = 2;
const SENT: u32 = 3;

fn control_let(id: u32, name: &str, ty: Type, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty,
        mutable: true,
        init: Some(init),
    }
}

/// The activation-frame shape the async-to-generator transform emits: one
/// `PreallocateBoxes` covering the control cells and body locals.
fn activation_frame() -> Vec<Stmt> {
    vec![
        Stmt::PreallocateBoxes(vec![STATE, DONE, SENT]),
        control_let(STATE, "__gen_state", Type::Number, Expr::Number(0.0)),
        control_let(DONE, "__gen_done", Type::Boolean, Expr::Bool(false)),
        control_let(SENT, "__gen_sent", Type::Any, Expr::Undefined),
    ]
}

/// Kind selection on local slots: each cell routes to ITS release entry
/// point, mirroring `emit_preallocate_boxes`' classification exactly.
#[test]
fn release_boxes_lowers_each_kind_to_its_entry_point() {
    let mut body = activation_frame();
    body.push(Stmt::ReleaseBoxes(vec![STATE, DONE, SENT]));
    body.push(Stmt::Return(Some(Expr::Undefined)));
    let ir = ir_for_fn_body("release_kinds", body);
    assert!(
        ir.contains("call void @js_i32_box_release("),
        "__gen_state (i32 control cell) must release through js_i32_box_release:\n{ir}"
    );
    assert!(
        ir.contains("call void @js_bool_box_release("),
        "__gen_done (i1 control cell) must release through js_bool_box_release:\n{ir}"
    );
    assert!(
        ir.contains("call void @js_box_release("),
        "__gen_sent (JSValue cell) must release through js_box_release:\n{ir}"
    );
}

/// The real emission site: the release lives in the synthesized step
/// closure's terminal arms, where the cells arrive as CAPTURES. The lowering
/// must fetch the box pointer from the capture slot
/// (`js_closure_get_capture_bits`) and still emit all three release calls.
#[test]
fn release_boxes_lowers_through_closure_captures() {
    let mut body = activation_frame();
    body.push(Stmt::Expr(Expr::Closure {
        func_id: 900,
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![
            // Reference a cell so the body is not trivially empty.
            Stmt::Expr(Expr::LocalSet(SENT, Box::new(Expr::Undefined))),
            Stmt::ReleaseBoxes(vec![STATE, DONE, SENT]),
            Stmt::Return(Some(Expr::Undefined)),
        ],
        captures: vec![STATE, DONE, SENT],
        mutable_captures: vec![STATE, DONE, SENT],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_strict: false,
        is_async: false,
        is_generator: false,
    }));
    body.push(Stmt::Return(Some(Expr::Undefined)));
    let ir = ir_for_fn_body("release_captures", body);
    for call in [
        "call void @js_box_release(",
        "call void @js_i32_box_release(",
        "call void @js_bool_box_release(",
    ] {
        assert!(
            ir.contains(call),
            "release must lower inside the step closure (missing `{call}`):\n{ir}"
        );
    }
}

/// A release with no visible cell must be a silent skip, not an error: the
/// statement is a reclamation hint (here, id 99 has no slot and no capture).
#[test]
fn release_of_an_unknown_id_is_skipped() {
    let mut body = activation_frame();
    body.push(Stmt::ReleaseBoxes(vec![99]));
    body.push(Stmt::Return(Some(Expr::Undefined)));
    let ir = ir_for_fn_body("release_unknown", body);
    assert!(
        !ir.contains("call void @js_box_release("),
        "an id with no cell must not emit a release:\n{ir}"
    );
}
