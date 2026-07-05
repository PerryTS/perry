use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{
    ArrayElement, BinaryOp, CompareOp, Expr, Function, Interface, InterfaceProperty, Module,
    ModuleInitKind, Stmt, UpdateOp,
};
use perry_types::{ObjectType, PropertyInfo, Type};

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvVarGuard {
    key: &'static str,
    prev: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: Option<&str>) -> Self {
        let prev = std::env::var_os(key);
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
        Self { key, prev }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

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
        nextjs_path_init_modules: Vec::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}

fn prop(ty: Type) -> PropertyInfo {
    PropertyInfo {
        ty,
        optional: false,
        readonly: false,
    }
}

fn object_type(fields: &[(&str, Type)]) -> Type {
    let mut properties = std::collections::HashMap::new();
    let mut property_order = Vec::new();
    for (name, ty) in fields {
        properties.insert((*name).to_string(), prop(ty.clone()));
        property_order.push((*name).to_string());
    }
    Type::Object(ObjectType {
        name: None,
        properties,
        property_order: Some(property_order),
        index_signature: None,
    })
}

fn base_module(name: &str, body: Vec<Stmt>, interfaces: Vec<Interface>) -> Module {
    Module {
        name: name.to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces,
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
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
        }],
        init: Vec::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
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

fn ir_for(module: Module) -> String {
    String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap()
}

fn block_between<'a>(ir: &'a str, start: &str, end: &str) -> &'a str {
    let start_pos = ir
        .find(start)
        .unwrap_or_else(|| panic!("IR should contain block marker {start}"));
    let block_ir = &ir[start_pos + 1..];
    let end_pos = block_ir
        .find(end)
        .unwrap_or_else(|| panic!("IR block starting at {start} should precede {end}"));
    &block_ir[..end_pos]
}

#[test]
fn scalar_object_literal_skips_pure_unobserved_initializers() {
    let module = base_module(
        "scalar_object_literal_used_fields.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "obj".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Object(vec![
                    ("used".to_string(), Expr::Integer(7)),
                    (
                        "unused".to_string(),
                        Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(Expr::String("drop_".to_string())),
                            right: Box::new(Expr::Integer(1)),
                        },
                    ),
                ])),
            },
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "used".to_string(),
            })),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);
    assert!(
        !ir.contains("call i64 @js_object_alloc"),
        "non-escaping object literal should stay scalar-replaced"
    );
    assert!(
        !ir.contains("call i64 @js_string_concat"),
        "pure unobserved field initializer should not be lowered"
    );
}

#[test]
fn scalar_array_literal_skips_pure_unobserved_initializers() {
    let module = base_module(
        "scalar_array_literal_used_indices.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "arr".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Array(vec![
                    Expr::Integer(7),
                    Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::String("drop_".to_string())),
                        right: Box::new(Expr::Integer(1)),
                    },
                    Expr::Integer(9),
                ])),
            },
            Stmt::Return(Some(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(0)),
            })),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);
    assert!(
        !ir.contains("call i64 @js_array_alloc"),
        "non-escaping array literal should stay scalar-replaced"
    );
    assert!(
        !ir.contains("call i64 @js_string_concat"),
        "pure unobserved array initializer should not be lowered"
    );
}

#[test]
fn scalar_object_literal_keeps_observable_unobserved_initializers() {
    let module = base_module(
        "scalar_object_literal_observable_unused.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "obj".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Object(vec![
                    ("used".to_string(), Expr::Integer(7)),
                    ("unused".to_string(), Expr::DateNow),
                ])),
            },
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "used".to_string(),
            })),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);
    assert!(
        ir.contains("call double @js_date_now"),
        "unobserved initializers that are not proven discardable must still be lowered"
    );
}

#[test]
fn scalar_object_literal_keeps_initializers_read_by_update() {
    let module = base_module(
        "scalar_object_literal_update_read.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "obj".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Object(vec![(
                    "count".to_string(),
                    Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::String("4".to_string())),
                        right: Box::new(Expr::Integer(1)),
                    },
                )])),
            },
            Stmt::Return(Some(Expr::PropertyUpdate {
                object: Box::new(Expr::LocalGet(1)),
                property: "count".to_string(),
                op: BinaryOp::Add,
                prefix: false,
            })),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);
    assert!(
        ir.contains("call i64 @js_string_concat"),
        "field initializer read by obj.field++ must still be lowered"
    );
}

fn assert_typed_feedback_setter_after(ir: &str, start_pos: usize, context: &str) {
    let after_start = &ir[start_pos..];
    assert!(
        after_start.contains("call void @js_typed_feedback_object_set_field_by_name"),
        "{context} should use the typed-feedback setter wrapper"
    );
    assert!(
        ir.contains("js_object_set_field_by_name"),
        "{context} should keep the safe runtime setter as the typed-feedback fallback"
    );
    assert!(
        !after_start.contains("call void @js_object_set_unboxed_f64_field"),
        "{context} should not use the raw unboxed field setter for dynamic mutation"
    );
}

fn point_module(name: &str, body: Vec<Stmt>) -> Module {
    base_module(name, body, Vec::new())
}

#[test]
fn numeric_array_literal_avoids_layout_note_calls() {
    let module = base_module(
        "numeric_array_literal.ts",
        vec![Stmt::Return(Some(Expr::Array(vec![
            Expr::Number(1.0),
            Expr::Number(2.0),
            Expr::Number(3.0),
        ])))],
        Vec::new(),
    );

    let ir = ir_for(module);
    assert!(
        !ir.contains("call void @js_gc_note_slot_layout"),
        "all-numeric array literals should keep the initial pointer-free layout without slot notes"
    );
}

#[test]
fn mixed_array_literal_emits_layout_note_calls() {
    let module = base_module(
        "mixed_array_literal.ts",
        vec![Stmt::Return(Some(Expr::Array(vec![
            Expr::Number(1.0),
            Expr::String("heap".to_string()),
        ])))],
        Vec::new(),
    );

    let ir = ir_for(module);
    assert!(
        ir.contains("call void @js_gc_note_slot_layout"),
        "mixed array literals should note pointer-bearing slots"
    );
}

#[test]
fn number_typed_local_array_literal_keeps_runtime_layout_note() {
    let module = base_module(
        "number_typed_local_array_literal.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "v".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::String("heap".to_string())),
            },
            Stmt::Return(Some(Expr::Array(vec![Expr::LocalGet(1)]))),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);
    assert!(
        ir.contains("call void @js_gc_note_slot_layout"),
        "a number-typed local is not runtime proof that the value is pointer-free"
    );
}

#[test]
fn number_typed_local_array_push_keeps_layout_note_and_barrier() {
    let module = base_module(
        "number_typed_local_array_push.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "v".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::String("heap".to_string())),
            },
            Stmt::Let {
                id: 2,
                name: "arr".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: true,
                init: Some(Expr::Array(Vec::new())),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: 2,
                value: Box::new(Expr::LocalGet(1)),
            }),
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);

    assert!(
        ir.contains("call i32 @js_typed_feedback_numeric_array_push_guard"),
        "number-typed array pushes should validate the runtime value before the numeric path"
    );
    assert!(
        ir.contains("call void @js_typed_feedback_record_fallback_call")
            && ir.contains("call i64 @js_array_push_f64"),
        "wrong runtime values must keep a boxed runtime push fallback"
    );
}

#[test]
fn c262_array_spread_uses_strict_iterator_append_and_preserves_holes() {
    let module = base_module(
        "c262_array_spread.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "iter".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Array(vec![Expr::Number(2.0)])),
            },
            Stmt::Return(Some(Expr::ArraySpread(vec![
                ArrayElement::Expr(Expr::Number(1.0)),
                ArrayElement::Hole,
                ArrayElement::Spread(Expr::LocalGet(1)),
            ]))),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);
    assert!(
        ir.contains("call i64 @js_array_push_hole"),
        "elisions should append a hole sentinel, not undefined"
    );
    assert!(
        ir.contains("call i64 @js_array_spread_append"),
        "spread operands should go through strict iterator materialization"
    );
}

#[test]
fn c262_array_has_own_property_uses_object_prototype_dispatch() {
    let module = base_module(
        "c262_array_has_own_property.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "arr".to_string(),
                ty: Type::Array(Box::new(Type::Any)),
                mutable: false,
                init: Some(Expr::Array(vec![Expr::Bool(true)])),
            },
            Stmt::Return(Some(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: "hasOwnProperty".to_string(),
                }),
                args: vec![Expr::String("0".to_string())],
                type_args: vec![],
                byte_offset: 0,
            })),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);
    assert!(
        ir.contains("call double @js_typed_feedback_native_call_method"),
        "array hasOwnProperty should dispatch through Object.prototype semantics"
    );
}

#[test]
fn c262_addition_assignment_operands_use_dynamic_add_helper() {
    let module = base_module(
        "c262_addition_order.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "y".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Number(0.0)),
            },
            Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalSet(1, Box::new(Expr::Number(1.0)))),
                right: Box::new(Expr::LocalGet(1)),
            })),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);
    assert!(
        ir.contains("call double @js_dynamic_string_or_number_add"),
        "addition with assignment/GetValue operands should preserve ToPrimitive ordering"
    );
}

#[test]
fn bounded_integer_array_store_omits_layout_note_and_barrier() {
    let module = base_module(
        "bounded_integer_array_store.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "arr".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: true,
                init: Some(Expr::Array(vec![
                    Expr::Number(0.0),
                    Expr::Number(0.0),
                    Expr::Number(0.0),
                ])),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 2,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(1)),
                        property: "length".to_string(),
                    }),
                }),
                update: Some(Expr::Update {
                    id: 2,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                // `arr[i] = i + 1`: integer-classified but neither iota nor
                // constant, so the #4957 bulk-fill matcher
                // (`match_numeric_bulk_fill_loop`) does NOT fire and the loop
                // keeps exercising the per-element guarded store path this
                // test is about. The plain `arr[i] = i` shape now lowers to
                // `js_array_fill_f64_iota_len_extend` — covered by
                // `numeric_iota_fill_loop_uses_bulk_helper_without_barrier`
                // below.
                body: vec![Stmt::Expr(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::LocalGet(2)),
                    value: Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Integer(1)),
                    }),
                })],
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);

    assert!(
        ir.contains("idxset.bounded_numeric_fast") && ir.contains("store double"),
        "bounded numeric array store should inline the guarded raw-f64 payload store"
    );
    assert!(
        !ir.contains("call i32 @js_array_numeric_set_f64_unboxed"),
        "bounded numeric array store should not call the redundant raw-f64 set helper"
    );
    assert!(
        ir.contains("call i32 @js_typed_feedback_numeric_array_index_set_guard"),
        "bounded numeric array stores must guard that the runtime layout is still raw-f64"
    );
    assert!(
        ir.contains("call double @js_typed_feedback_array_index_set_fallback_boxed"),
        "guarded bounded stores need a boxed fallback when the array downgraded"
    );
    assert!(
        !ir.contains("call void @js_gc_note_slot_layout"),
        "integer store into a numeric array should not update slot layout"
    );
    assert!(
        !ir.contains("call void @js_write_barrier_slot"),
        "integer store into a numeric array should not emit a slot barrier"
    );
}

#[test]
fn numeric_iota_fill_loop_uses_bulk_helper_without_barrier() {
    // #4957 lowers `for (let i = 0; i < arr.length; i++) arr[i] = i` over a
    // numeric array to a bulk iota fill instead of per-element guarded
    // stores. Pin that lowering — and that the bulk path, like the
    // per-element one, emits no layout note and no slot barrier for raw-f64
    // payloads.
    let module = base_module(
        "numeric_iota_fill_loop.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "arr".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: true,
                init: Some(Expr::Array(vec![
                    Expr::Number(0.0),
                    Expr::Number(0.0),
                    Expr::Number(0.0),
                ])),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 2,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(1)),
                        property: "length".to_string(),
                    }),
                }),
                update: Some(Expr::Update {
                    id: 2,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::LocalGet(2)),
                    value: Box::new(Expr::LocalGet(2)),
                })],
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);

    assert!(
        ir.contains("call i64 @js_array_fill_f64_iota_len_extend"),
        "iota fill over `arr.length` should lower to the bulk iota helper"
    );
    assert!(
        !ir.contains("call i32 @js_array_numeric_set_f64_unboxed"),
        "bulk-fill loop should not also emit per-element raw-f64 stores"
    );
    assert!(
        !ir.contains("call void @js_gc_note_slot_layout"),
        "bulk iota fill of a numeric array should not update slot layout"
    );
    assert!(
        !ir.contains("call void @js_write_barrier_slot"),
        "bulk iota fill of a numeric array should not emit a slot barrier"
    );
}

#[test]
fn integer_arithmetic_array_push_omits_inbounds_layout_note_and_barrier() {
    let module = base_module(
        "integer_arithmetic_array_push.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "arr".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: true,
                init: Some(Expr::Array(Vec::new())),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 2,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::Integer(8)),
                }),
                update: Some(Expr::Update {
                    id: 2,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::ArrayPush {
                    array_id: 1,
                    value: Box::new(Expr::Binary {
                        op: BinaryOp::Mul,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Number(1.5)),
                    }),
                })],
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);
    let fast_ir = block_between(&ir, "\napush.numeric_fast.", "\napush.numeric_fallback.");

    assert!(
        ir.contains("call i32 @js_typed_feedback_numeric_array_push_guard"),
        "plain-number loop pushes must guard that the runtime layout is still raw-f64"
    );
    assert!(
        ir.contains("call i64 @js_array_numeric_push_f64_unboxed"),
        "plain-number loop pushes should use the raw-f64 push helper on the guarded fast path"
    );
    assert!(
        !fast_ir.contains("call void @js_gc_note_slot_layout"),
        "integer arithmetic push value should not update slot layout"
    );
    assert!(
        !fast_ir.contains("call void @js_write_barrier_slot"),
        "integer arithmetic push value should not emit a slot barrier"
    );
}

#[test]
fn pointer_store_into_numeric_array_keeps_layout_note_and_barrier() {
    let module = base_module(
        "pointer_store_into_numeric_array.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "child".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Object(Vec::new())),
            },
            Stmt::Let {
                id: 2,
                name: "arr".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: true,
                init: Some(Expr::Array(vec![Expr::Number(0.0)])),
            },
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(2)),
                index: Box::new(Expr::Integer(0)),
                value: Box::new(Expr::LocalGet(1)),
            }),
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);
    let inbounds_ir = block_between(&ir, "\nidxset.inbounds.", "\nidxset.check_cap.");

    assert!(
        inbounds_ir.contains("call void @js_gc_note_slot_layout"),
        "pointer stores into statically numeric arrays must update slot layout"
    );
    assert!(
        inbounds_ir.contains("call void @js_write_barrier_slot"),
        "pointer stores into statically numeric arrays must emit slot barriers"
    );
}

#[test]
fn typed_object_literal_stable_path_installs_pointer_mask_descriptor() {
    let child_ty = object_type(&[("leaf", Type::Number)]);
    let row_iface = Interface {
        id: 1,
        name: "Row".to_string(),
        type_params: Vec::new(),
        extends: Vec::new(),
        properties: vec![
            InterfaceProperty {
                name: "id".to_string(),
                ty: Type::Number,
                optional: false,
                readonly: false,
            },
            InterfaceProperty {
                name: "active".to_string(),
                ty: Type::Boolean,
                optional: false,
                readonly: false,
            },
            InterfaceProperty {
                name: "child".to_string(),
                ty: child_ty.clone(),
                optional: false,
                readonly: false,
            },
        ],
        methods: Vec::new(),
        is_exported: false,
    };
    let module = base_module(
        "typed_shape_literal.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "child".to_string(),
                ty: child_ty,
                mutable: false,
                init: Some(Expr::Object(vec![("leaf".to_string(), Expr::Number(1.0))])),
            },
            Stmt::Let {
                id: 2,
                name: "row".to_string(),
                ty: Type::Named("Row".to_string()),
                mutable: false,
                init: Some(Expr::Object(vec![
                    ("id".to_string(), Expr::Number(7.0)),
                    ("active".to_string(), Expr::Bool(true)),
                    ("child".to_string(), Expr::LocalGet(1)),
                ])),
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
        vec![row_iface],
    );

    let ir = ir_for(module);
    assert!(
        ir.contains("call i64 @js_object_alloc_with_shape"),
        "fixture should use the stable object-literal shape allocator"
    );
    assert!(
        ir.contains("@perry_typed_obj_shape_raw_f64_mask_"),
        "typed object literal should emit a raw-f64 mask constant"
    );
    assert!(
        ir.contains("@perry_typed_obj_shape_ptr_mask_"),
        "typed object literal should emit a pointer-mask constant"
    );
    assert!(
        ir.contains("constant [1 x i64] [i64 1]"),
        "only the id slot (slot 0) should be raw-f64"
    );
    assert!(
        ir.contains("constant [1 x i64] [i64 4]"),
        "only the child slot (slot 2) should be pointer-bearing"
    );

    let mask_call_pos = ir
        .find("ptr @perry_typed_obj_shape_ptr_mask_")
        .expect("typed descriptor call should reference the object-literal mask");
    let before_mask_call = &ir[..mask_call_pos];
    let alloc_pos = before_mask_call
        .rfind("call i64 @js_object_alloc_with_shape")
        .expect("descriptor should belong to an object-literal allocation");
    let set_pos = before_mask_call
        .rfind("call void @js_object_set_field")
        .expect("object literal should initialize fields before installing descriptor");
    assert!(alloc_pos < set_pos);
}

#[test]
fn typed_object_literal_pointer_free_descriptor_precedes_dynamic_mutation() {
    let row_ty = object_type(&[("count", Type::Number)]);
    let module = base_module(
        "typed_shape_mutation.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "row".to_string(),
                ty: row_ty,
                mutable: true,
                init: Some(Expr::Object(vec![("count".to_string(), Expr::Number(1.0))])),
            },
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "count".to_string(),
                value: Box::new(Expr::String("now-pointer".to_string())),
            }),
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);
    let descriptor_pos = ir
        .find("call void @js_gc_init_typed_shape_layout")
        .expect("number-only object type should install a pointer-free descriptor");
    assert!(
        ir.contains("@perry_typed_obj_shape_raw_f64_mask_"),
        "number-only object type should install a raw-f64 descriptor mask"
    );
    assert_typed_feedback_setter_after(
        &ir,
        descriptor_pos,
        "dynamic property mutation after a pointer-free descriptor",
    );
}

#[test]
fn unboxed_point_literal_gate_on_emits_raw_setters_and_pointer_free_layout() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _env = EnvVarGuard::set("PERRY_UNBOXED_OBJECT_FIELDS", Some("1"));
    let point_ty = object_type(&[("x", Type::Number), ("y", Type::Number)]);
    let module = point_module(
        "unboxed_point_on.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "p".to_string(),
                ty: point_ty,
                mutable: false,
                init: Some(Expr::Object(vec![
                    ("x".to_string(), Expr::Number(1.5)),
                    ("y".to_string(), Expr::Number(2.5)),
                ])),
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    );

    let ir = ir_for(module);
    assert!(ir.contains("call i64 @js_object_alloc_with_shape"));
    assert!(ir.contains("call void @js_object_set_unboxed_f64_field"));
    assert!(ir.contains("call void @js_gc_init_unboxed_object_layout"));
    assert!(
        ir.contains("i32 2, i64 3, i64 0"),
        "unboxed point layout should install raw f64 slots for x/y and no pointer slots"
    );
    assert!(
        !ir.contains("call void @js_gc_init_typed_shape_layout"),
        "gate-on exact point literals should use the unboxed layout installer"
    );
}

#[test]
fn unboxed_point_literal_gate_off_uses_existing_typed_shape_path() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _env = EnvVarGuard::set("PERRY_UNBOXED_OBJECT_FIELDS", None);
    let point_ty = object_type(&[("x", Type::Number), ("y", Type::Number)]);
    let module = point_module(
        "unboxed_point_off.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "p".to_string(),
                ty: point_ty,
                mutable: false,
                init: Some(Expr::Object(vec![
                    ("x".to_string(), Expr::Number(1.5)),
                    ("y".to_string(), Expr::Number(2.5)),
                ])),
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    );

    let ir = ir_for(module);
    assert!(ir.contains("call i64 @js_object_alloc_with_shape"));
    assert!(ir.contains("call void @js_object_set_field"));
    assert!(ir.contains("call void @js_gc_init_typed_shape_layout"));
    assert!(ir.contains("@perry_typed_obj_shape_raw_f64_mask_"));
    assert!(ir.contains("constant [1 x i64] [i64 3]"));
    assert!(!ir.contains("call void @js_object_set_unboxed_f64_field"));
    assert!(!ir.contains("call void @js_gc_init_unboxed_object_layout"));
}

#[test]
fn unboxed_point_dynamic_mutation_still_uses_safe_by_name_setter() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _env = EnvVarGuard::set("PERRY_UNBOXED_OBJECT_FIELDS", Some("1"));
    let point_ty = object_type(&[("x", Type::Number), ("y", Type::Number)]);
    let module = point_module(
        "unboxed_point_mutation.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "p".to_string(),
                ty: point_ty,
                mutable: true,
                init: Some(Expr::Object(vec![
                    ("x".to_string(), Expr::Number(1.0)),
                    ("y".to_string(), Expr::Number(2.0)),
                ])),
            },
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
                value: Box::new(Expr::String("heap".to_string())),
            }),
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    );

    let ir = ir_for(module);
    let layout_pos = ir
        .find("call void @js_gc_init_unboxed_object_layout")
        .expect("fixture should install unboxed layout");
    assert_typed_feedback_setter_after(
        &ir,
        layout_pos,
        "dynamic property mutation after an unboxed layout",
    );
}
