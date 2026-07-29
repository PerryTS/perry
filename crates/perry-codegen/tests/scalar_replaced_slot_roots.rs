//! #6968: a heap value stored into a scalar-replaced object field or array
//! element must be a precise GC root.
//!
//! Scalar replacement deletes the object and keeps one entry-block alloca per
//! field. Those allocas belong to no HIR local, so the pre-lowering
//! `collect_pointer_typed_locals` pass — which assigns shadow slots by walking
//! `Stmt::Let` — cannot see them, and nothing bound them. A collection landing
//! between the store and the read swept the value out from under the alloca.
//!
//! The end-to-end proof is `test-files/test_gap_repsel_scalar_replaced_locals.ts`
//! on the `cons_scan_off` arm of `scripts/gc_repsel_matrix.sh` — the only
//! configuration where the bug is observable, because every automatic
//! collection otherwise forces a conservative native-stack scan that pins the
//! alloca by accident. These tests pin the *codegen contract* that arm depends
//! on, in-process, so a lowering path that goes back to emitting a bare alloca
//! fails here instead of under a later narrowing of the forced scan.
//!
//! Both directions are covered: the gate must stay silent for a
//! literal whose fields are numbers, or every scalar-replaced `{x, y}` in a hot
//! loop would pay for rooting a value that can never be collected (#6997).

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

fn let_stmt(id: u32, name: &str, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

/// A heap value with no other reference: a fresh object literal that is not
/// itself bound to a local, so it is allocated on the heap and the field
/// alloca is the only thing pointing at it.
fn heap_value() -> Expr {
    Expr::Object(vec![("k".to_string(), Expr::Number(1.0))])
}

fn field_get(local: u32, field: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(local)),
        property: field.to_string(),
        byte_offset: 0,
    }
}

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

/// Count of emitted `js_shadow_slot_bind` CALL sites. The `declare` line is
/// unconditional, so only calls count.
fn bind_calls(ir: &str) -> usize {
    ir.matches("call void @js_shadow_slot_bind(").count()
}

/// The slot count baked into this module-init function's frame push.
fn frame_slot_count(ir: &str) -> u32 {
    let needle = "call i64 @js_shadow_frame_push(i32 ";
    let start = ir
        .find(needle)
        .map(|i| i + needle.len())
        .unwrap_or_else(|| {
            panic!("expected a shadow frame push in:\n{ir}");
        });
    let rest = &ir[start..];
    let end = rest.find(')').expect("malformed frame push");
    rest[..end]
        .parse()
        .expect("frame push count is not a number")
}

/// A scalar-replaced object literal whose field holds a heap value must bind
/// that field's alloca as a precise root.
///
/// Pre-fix the field store was `store double %v, ptr %slot` into a bare
/// entry-block alloca with no `js_shadow_slot_bind` anywhere in the function:
/// the object local's own reserved slot is only ever *cleared*, because scalar
/// replacement leaves no object handle to bind. A collection between the store
/// and the read therefore swept the value (#6968).
#[test]
fn scalar_replaced_object_field_holding_a_heap_value_is_bound() {
    let ir = ir_for(
        "scalar_object_field_root.ts",
        vec![
            let_stmt(
                1,
                "o",
                Expr::Object(vec![
                    ("a".to_string(), heap_value()),
                    ("b".to_string(), Expr::Number(2.0)),
                ]),
            ),
            console_log(vec![field_get(1, "a"), field_get(1, "b")]),
        ],
    );

    assert!(
        bind_calls(&ir) > 0,
        "the scalar-replaced field alloca holding a heap value must be bound \
         as a precise root (#6968):\n{ir}"
    );

    // Frame growth, stated DIFFERENTIALLY. `frame_slot_count(&ir) > 0` on its
    // own certifies nothing: `o` is pointer-typed, so the pre-lowering pass
    // already reserved it a slot and the frame is non-empty with or without
    // this fix. The claim that has teeth is that the pointer-capable field
    // takes an ADDITIONAL slot the pointer analysis could not have predicted,
    // so compare against the structurally identical numeric-only literal —
    // same local, same field count, same reads, no rooting.
    let control = ir_for(
        "scalar_object_field_root_control.ts",
        vec![
            let_stmt(
                1,
                "o",
                Expr::Object(vec![
                    ("a".to_string(), Expr::Number(1.0)),
                    ("b".to_string(), Expr::Number(2.0)),
                ]),
            ),
            console_log(vec![field_get(1, "a"), field_get(1, "b")]),
        ],
    );
    assert!(
        frame_slot_count(&ir) > frame_slot_count(&control),
        "binding a scalar-replacement alloca must grow the shadow frame beyond \
         what the pre-lowering pointer analysis reserved: heap-field literal \
         has {} slots, the numeric-only control has {} — the pre-lowering pass \
         cannot see these allocas, so the extra slot can only come from \
         `reserve_shadow_slot` (#6968):\n{ir}",
        frame_slot_count(&ir),
        frame_slot_count(&control),
    );
}

/// The gate, from the other side: a literal whose every field is a number
/// must emit no rooting at all.
///
/// This is the #6997 lesson — rooting a value that can never be collected is
/// pure cost on the path that exists *because* it was optimized. The decision
/// is made from the lowering (`expr_is_known_non_pointer_shadow_value`), not
/// from a declared type, so it holds for `any`-typed locals too — which is
/// exactly what this module builds (`Type::Any`).
#[test]
fn numeric_only_scalar_replaced_object_emits_no_rooting() {
    let ir = ir_for(
        "scalar_object_numeric.ts",
        vec![
            let_stmt(
                1,
                "p",
                Expr::Object(vec![
                    ("x".to_string(), Expr::Number(1.0)),
                    ("y".to_string(), Expr::Number(2.0)),
                ]),
            ),
            console_log(vec![field_get(1, "x"), field_get(1, "y")]),
        ],
    );

    assert_eq!(
        bind_calls(&ir),
        0,
        "a scalar-replaced literal with only numeric fields must not pay for \
         GC rooting:\n{ir}"
    );
}

/// The array-literal form of the same defect: `const a = [heap, n]` becomes
/// one alloca per element, and element 0 is the only reference to its value.
#[test]
fn scalar_replaced_array_element_holding_a_heap_value_is_bound() {
    let ir = ir_for(
        "scalar_array_element_root.ts",
        vec![
            let_stmt(1, "a", Expr::Array(vec![heap_value(), Expr::Number(2.0)])),
            console_log(vec![
                Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::Integer(0)),
                },
                Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::Integer(1)),
                },
            ]),
        ],
    );

    assert!(
        bind_calls(&ir) > 0,
        "the scalar-replaced array element alloca holding a heap value must be \
         bound as a precise root (#6968):\n{ir}"
    );
}

/// …and its numeric twin stays free.
#[test]
fn numeric_only_scalar_replaced_array_emits_no_rooting() {
    let ir = ir_for(
        "scalar_array_numeric.ts",
        vec![
            let_stmt(
                1,
                "a",
                Expr::Array(vec![Expr::Number(1.0), Expr::Number(2.0)]),
            ),
            console_log(vec![Expr::IndexGet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(0)),
            }]),
        ],
    );

    assert_eq!(
        bind_calls(&ir),
        0,
        "a scalar-replaced numeric array literal must not pay for GC rooting:\n{ir}"
    );
}

/// The scalar-replaced `split()` arm: its element slots receive
/// `js_string_split_part_value` results — fresh heap strings with nothing else
/// referring to them — so they are rooted unconditionally (there is no HIR
/// expression to gate on; the value is synthesized by codegen).
///
/// Stated differentially against the same string local WITHOUT the split,
/// because the string local itself is pointer-typed and binds its own slot in
/// both compilers: only the extra binds can come from the part slots.
#[test]
fn scalar_replaced_split_parts_are_bound() {
    let source = Stmt::Let {
        id: 1,
        name: "s".to_string(),
        ty: Type::String,
        mutable: false,
        init: Some(Expr::String("a,b,c".to_string())),
    };
    let split = Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: "split".to_string(),
            byte_offset: 0,
        }),
        args: vec![Expr::String(",".to_string())],
        type_args: Vec::new(),
        byte_offset: 0,
    };
    let ir = ir_for(
        "scalar_split_parts.ts",
        vec![
            source.clone(),
            let_stmt(2, "parts", split),
            console_log(vec![
                Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(2)),
                    index: Box::new(Expr::Integer(0)),
                },
                Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(2)),
                    index: Box::new(Expr::Integer(1)),
                },
            ]),
        ],
    );
    let control = ir_for(
        "scalar_split_parts_control.ts",
        vec![source, console_log(vec![Expr::LocalGet(1)])],
    );

    assert!(
        bind_calls(&ir) > bind_calls(&control),
        "the scalar-replaced split part slots must be bound as precise roots: \
         split IR has {} binds, the split-free control has {} (#6968):\n{ir}",
        bind_calls(&ir),
        bind_calls(&control),
    );
}

/// A later `o.a = <heap>` writes the same alloca and must root it too — the
/// object-literal initializer is not the only store site.
#[test]
fn later_store_into_a_scalar_replaced_field_is_bound() {
    let ir = ir_for(
        "scalar_object_field_reassign.ts",
        vec![
            let_stmt(
                1,
                "o",
                Expr::Object(vec![("a".to_string(), Expr::Number(0.0))]),
            ),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "a".to_string(),
                value: Box::new(heap_value()),
            }),
            console_log(vec![field_get(1, "a")]),
        ],
    );

    assert!(
        bind_calls(&ir) > 0,
        "a heap value assigned into a scalar-replaced field after construction \
         must be rooted as well (#6968):\n{ir}"
    );
}
