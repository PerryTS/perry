//! Object field stores keep string-alias and write-barrier bookkeeping while
//! ShapeId, rather than an object layout note, determines GC slot tracing.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Function, Module, ModuleInitKind, Param, Stmt};

const NOTE_CALL: &str = "call void @js_gc_note_slot_layout(";

fn ir_opts() -> CompileOptions {
    CompileOptions {
        static_shape_ids: Vec::new(),
        program_class_shape_ids: Default::default(),
        target: None,
        is_entry_module: true,
        non_entry_module_prefixes: Vec::new(),
        thread_literal_module_prefixes: Vec::new(),
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
        constructor_param_counts: Default::default(),
        imported_classes: Vec::new(),
        short_spread_method_candidates: std::sync::Arc::default(),
        program_class_accessor_names: Default::default(),
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
        program_is_synchronous: false,
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

fn field(name: &str, ty: Type) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

/// `class Link { next: Link | null; v: number }` — slot 0 pointer-masked, slot
/// 1 raw-f64-masked. The two slots are what make the positive and the negative
/// test differ in exactly one thing.
fn link_class() -> Class {
    Class {
        id: 303,
        name: "Link".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![
            field(
                "next",
                Type::Union(vec![Type::Named("Link".to_string()), Type::Null]),
            ),
            field("v", Type::Number),
            // Neither pointer-bearing nor a raw-f64 candidate, so slot 2 is in
            // NEITHER mask — the case where the note is what SETS the bit.
            field("flag", Type::Boolean),
        ],
        constructor: Some(link_ctor()),
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

const A_ID: u32 = 1;
const B_ID: u32 = 2;
const CTOR_V_ID: u32 = 3;
const X_ID: u32 = 4;

/// `constructor(v: number) { this.next = null; this.v = v }` — the canonical
/// linked-structure prologue, and the shape #7686 taught
/// `ctor_prologue_param_assigned_fields` to admit. Present so the class gets a
/// keys global and an at-allocation layout declaration, exactly as `cycles.ts`
/// does; without it the test would be asking its question of a shape the real
/// compiler never produces.
fn link_ctor() -> Function {
    Function {
        id: 900,
        name: "constructor".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: CTOR_V_ID,
            name: "v".to_string(),
            ty: Type::Number,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Void,
        body: vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "next".to_string(),
                value: Box::new(Expr::Null),
            }),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "v".to_string(),
                value: Box::new(Expr::LocalGet(CTOR_V_ID)),
            }),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

/// `function link(a: Link, b: Link) { a.<property> = <value> }`, called once
/// from module init so it is not dead.
///
/// The store lives in a FUNCTION with declared-`Link` parameters, not in module
/// init, because that is where `receiver_class_name` resolves a monomorphic
/// receiver — the same position `cycles.ts`'s `makeCycle` puts it in.
fn store_module(property: &str, value: Expr) -> Module {
    let mut m = Module::new("conforming_layout_note.ts");
    m.classes = vec![link_class()];
    let link = || Type::Named("Link".to_string());
    m.functions = vec![Function {
        id: 10,
        name: "link".to_string(),
        type_params: Vec::new(),
        params: vec![
            Param {
                id: A_ID,
                name: "a".to_string(),
                ty: link(),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
            Param {
                id: B_ID,
                name: "b".to_string(),
                ty: link(),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
        ],
        return_type: Type::Void,
        body: vec![Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::LocalGet(A_ID)),
            property: property.to_string(),
            value: Box::new(value),
        })],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    m.init = vec![
        Stmt::Let {
            id: X_ID,
            name: "x".to_string(),
            ty: link(),
            mutable: false,
            init: Some(Expr::New {
                class_name: "Link".to_string(),
                args: vec![Expr::Number(1.0)],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(10)),
            args: vec![Expr::LocalGet(X_ID), Expr::LocalGet(X_ID)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn emit(m: &Module) -> String {
    String::from_utf8(compile_module(m, ir_opts()).unwrap()).expect("LLVM IR should be UTF-8")
}

/// A pointer-valued store into a declared pointer slot must retain the
/// barrier while emitting no object layout note.
#[test]
fn pointer_slot_store_keeps_barrier_without_layout_note() {
    let ir = emit(&store_module("next", Expr::LocalGet(B_ID)));
    assert!(
        !ir.contains(NOTE_CALL),
        "object store emitted a layout note:\n{ir}"
    );
    assert!(
        ir.contains("class_field_set.barrier"),
        "pointer store lost its barrier arm:\n{ir}"
    );
}

/// A class field whose declared type is scalar can still receive a heap
/// pointer through dynamic JS. The emitted barrier must survive that case too.
#[test]
fn dynamically_pointer_valued_scalar_slot_keeps_barrier_without_layout_note() {
    let ir = emit(&store_module("flag", Expr::LocalGet(B_ID)));
    assert!(
        !ir.contains(NOTE_CALL),
        "object store emitted a layout note:\n{ir}"
    );
    assert!(
        ir.contains("class_field_set.barrier"),
        "dynamic pointer store lost its barrier arm:\n{ir}"
    );
}
