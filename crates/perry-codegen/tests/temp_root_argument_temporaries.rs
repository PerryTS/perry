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

/// …and it IS emitted when an earlier element is a heap value and a later one
/// allocates: that value sits in an SSA register across the allocation, which
/// is not a root.
#[test]
fn array_literal_roots_elements_before_an_allocating_element() {
    let ir = ir_for(
        "array_literal_gc.ts",
        // Element 0 is itself a heap value (a literal would be skipped: it
        // loads from a module global that is already a registered GC root).
        vec![Stmt::Expr(Expr::Array(vec![allocating(), allocating()]))],
    );

    assert!(
        ir.contains("call i32 @js_gc_temp_root_push"),
        "an array literal with an allocating later element must root the \
         heap elements already evaluated (#6951):\n{ir}"
    );
}

// ---------------------------------------------------------------------------
// #6996: the gate must not root values that cannot be heap references.
//
// `native_abi_packet_control`'s kernel is `(buf[i] + packet.tag + i) & 255`.
// The left operand is a byte; the right is a property get on an `any`, so the
// add lowers through `js_dynamic_string_or_number_add` and the operand pair is
// rooted across the property get. Rooting the byte protected nothing and cost
// a push / re-read / truncate on every one of the loop's 262 144 iterations —
// it turned the `hot_loops_no_runtime_calls` contract red.

/// A local declared with a Buffer annotation, so the receiver of the element
/// read is a plain local get (the shape the fixture's parameter has).
fn buffer_local(id: u32) -> Stmt {
    Stmt::Let {
        id,
        name: "buf".to_string(),
        ty: perry_hir::types::Type::Named("Buffer".to_string()),
        mutable: false,
        init: Some(Expr::Undefined),
    }
}

/// An `any` local, so a property get on it is neither numeric-proven nor
/// allocation-free — exactly what forces the dynamic add and its rooting.
fn any_local(id: u32) -> Stmt {
    Stmt::Let {
        id,
        name: "packet".to_string(),
        ty: perry_hir::types::Type::Any,
        mutable: false,
        init: Some(Expr::Undefined),
    }
}

fn packet_tag(id: u32) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(id)),
        property: "tag".to_string(),
        byte_offset: 0,
    }
}

/// `<element read> + packet.tag`, bound to a local so nothing folds it away.
fn dynamic_add_with_element_read(element: Expr, packet_id: u32) -> Stmt {
    Stmt::Let {
        id: 90,
        name: "next".to_string(),
        ty: perry_hir::types::Type::Number,
        mutable: false,
        init: Some(Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left: Box::new(element),
            right: Box::new(packet_tag(packet_id)),
        }),
    }
}

/// A proven-index typed-array element read is a byte (or `undefined` out of
/// range) by construction, so it must not be rooted.
#[test]
fn buffer_element_operand_is_not_temp_rooted() {
    let ir = ir_for(
        "typed_element_operand.ts",
        vec![
            buffer_local(0),
            any_local(1),
            dynamic_add_with_element_read(
                Expr::Uint8ArrayGet {
                    array: Box::new(Expr::LocalGet(0)),
                    index: Box::new(Expr::Integer(0)),
                },
                1,
            ),
        ],
    );

    assert!(
        ir.contains("call double @js_dynamic_string_or_number_add"),
        "the test must actually reach the rooted operand-pair lowering:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_gc_temp_root_push"),
        "a proven-index typed-array element read is number-or-undefined by \
         construction — rooting it protects nothing and costs three runtime \
         calls per iteration (#6996):\n{ir}"
    );
}

/// Same for a `Buffer`-node element read, whose every lowering coerces the key
/// to i32 and reads a byte.
#[test]
fn buffer_index_get_operand_is_not_temp_rooted() {
    let ir = ir_for(
        "buffer_index_operand.ts",
        vec![
            buffer_local(0),
            any_local(1),
            dynamic_add_with_element_read(
                Expr::BufferIndexGet {
                    buffer: Box::new(Expr::LocalGet(0)),
                    index: Box::new(Expr::Integer(0)),
                },
                1,
            ),
        ],
    );

    assert!(
        !ir.contains("call i32 @js_gc_temp_root_push"),
        "a Buffer element read is a byte or `undefined` on every lowering \
         path (#6996):\n{ir}"
    );
}

/// The soundness boundary, held from the other side: a symbol key does NOT
/// read an element. It resolves through `js_object_get_symbol_property`, which
/// hands back a `%TypedArray%.prototype` accessor — a heap value that an
/// allocating sibling can sweep. It must still be rooted.
#[test]
fn symbol_keyed_typed_array_read_is_still_temp_rooted() {
    let ir = ir_for(
        "typed_symbol_key_operand.ts",
        vec![
            buffer_local(0),
            any_local(1),
            dynamic_add_with_element_read(
                Expr::Uint8ArrayGet {
                    array: Box::new(Expr::LocalGet(0)),
                    index: Box::new(Expr::SymbolFor(Box::new(Expr::String(
                        "Symbol.iterator".to_string(),
                    )))),
                },
                1,
            ),
        ],
    );

    assert!(
        ir.contains("call i32 @js_gc_temp_root_push"),
        "a symbol-keyed read returns a prototype accessor, not a byte — the \
         #6996 skip must not reach it:\n{ir}"
    );
}

/// The other excluded lowering: without the integer-array-index proof the read
/// goes to `js_typed_array_index_get_dynamic`, which falls through to
/// string-keyed property lookup, and an expando can hold anything.
#[test]
fn unproven_key_typed_array_read_is_still_temp_rooted() {
    let ir = ir_for(
        "typed_unproven_key_operand.ts",
        vec![
            buffer_local(0),
            any_local(1),
            Stmt::Let {
                id: 2,
                name: "k".to_string(),
                ty: perry_hir::types::Type::Any,
                mutable: false,
                init: Some(Expr::Undefined),
            },
            dynamic_add_with_element_read(
                Expr::Uint8ArrayGet {
                    array: Box::new(Expr::LocalGet(0)),
                    index: Box::new(Expr::LocalGet(2)),
                },
                1,
            ),
        ],
    );

    assert!(
        ir.contains("call i32 @js_gc_temp_root_push"),
        "an unproven key reads a property, not an element — the #6996 skip \
         must not reach it:\n{ir}"
    );
}
