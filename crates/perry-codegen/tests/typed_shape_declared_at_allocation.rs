//! #7510 item 1 / #7512 residual: the typed-shape layout of a qualifying class
//! is DECLARED at the allocation site instead of validated after the
//! constructor.
//!
//! The defect these lock down is an ordering one.
//! `js_gc_init_typed_shape_layout` was emitted after the constructor call, so
//! no raw-f64 class-field store *inside* the constructor could pass its
//! `GC_OBJ_TYPED_LAYOUT_INTACT` guard, and every one fell back to
//! `js_put_value_set`. #7512's stated acceptance is an IR census showing the
//! two real field stores of `constructor(v, w) { this.v = v; this.w = w }`
//! with `v: number; w: number` no longer routing through it. Its static half
//! is `the_declaration_dominates_the_constructor_call`; the rest is a runtime
//! property of the guarded store diamond (whose fallback arm is emitted either
//! way — what changed is whether its guard can pass), measured as 1.55× on
//! `push_cls`.
//!
//! The negative tests matter as much as the positive one: the declaration
//! skips the runtime's slot validation, so its whole soundness rests on the
//! gate refusing every shape where a read could observe a raw-f64 slot before
//! its first write, or where the collector's view at birth would not be
//! `POINTER_FREE`.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Function, Module, ModuleInitKind, Param, Stmt};

const DECLARE_CALL: &str = "call void @js_gc_declare_typed_shape_layout";
const INIT_CALL: &str = "call void @js_gc_init_typed_shape_layout";

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

fn param(id: u32, name: &str, ty: Type) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

/// `this.<name> = <param id>` in the shape user source lowers to
/// (`PutValueSet`, not the synthesized `PropertySet` — see #7512).
fn assign_field_from_param(name: &str, param_id: u32) -> Stmt {
    Stmt::Expr(Expr::PutValueSet {
        target: Box::new(Expr::This),
        key: Box::new(Expr::String(name.to_string())),
        value: Box::new(Expr::LocalGet(param_id)),
        receiver: Box::new(Expr::This),
        strict: false,
    })
}

fn ctor(params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id: 900,
        name: "constructor".to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Void,
        body,
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

fn class(name: &str, fields: Vec<ClassField>, constructor: Option<Function>) -> Class {
    Class {
        id: 1,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields,
        constructor,
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

fn module_with_new(class: Class, arg_count: usize) -> Module {
    let class_name = class.name.clone();
    Module {
        name: "typed_shape_declared_at_allocation.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: vec![class],
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Named(class_name.clone()),
            body: vec![Stmt::Return(Some(Expr::New {
                class_name,
                args: (0..arg_count).map(|i| Expr::Number(i as f64)).collect(),
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }))],
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
        local_source_spans: std::collections::HashMap::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

fn compile_ir(module: &Module) -> String {
    String::from_utf8(compile_module(module, empty_opts()).unwrap()).unwrap()
}

/// The motivating shape: `class Node { v: number; w: number;
/// constructor(v, w) { this.v = v; this.w = w } }`.
fn declarable_module() -> Module {
    module_with_new(
        class(
            "Node",
            vec![field("v", Type::Number), field("w", Type::Number)],
            Some(ctor(
                vec![param(10, "v", Type::Number), param(11, "w", Type::Number)],
                vec![
                    assign_field_from_param("v", 10),
                    assign_field_from_param("w", 11),
                ],
            )),
        ),
        2,
    )
}

#[test]
fn declarable_class_declares_its_layout_at_the_allocation_site() {
    let ir = compile_ir(&declarable_module());
    assert!(
        ir.contains(DECLARE_CALL),
        "a class whose constructor prologue assigns every number field from a \
         plain parameter must declare its layout at allocation:\n{ir}"
    );
}

/// The declaration and the post-constructor install are keyed on ONE predicate,
/// so exactly one of them is emitted. Two installs would pay back the cost this
/// removes; zero would leave the instance with no descriptor at all.
#[test]
fn the_declaration_replaces_the_post_constructor_install() {
    let ir = compile_ir(&declarable_module());
    assert!(
        !ir.contains(INIT_CALL),
        "the post-constructor install must be suppressed once the layout is \
         declared at allocation:\n{ir}"
    );
}

/// #7512's stated acceptance criterion. The constructor's own field stores must
/// stop routing through the by-name fallback — which is only possible if the
/// descriptor exists before the constructor body runs.
/// #7512's stated acceptance is that the constructor's own field stores stop
/// routing through `js_put_value_set`. That is a *runtime* property of the
/// guarded store diamond — the diamond's fallback arm is emitted either way;
/// what changed is whether its guard can pass. The static half of the claim,
/// and the whole of the codegen change, is this ordering: the declaration must
/// dominate the constructor call, because a descriptor installed after the
/// constructor arrives after the only stores that wanted it.
#[test]
fn the_declaration_dominates_the_constructor_call() {
    let ir = compile_ir(&declarable_module());
    let alloc_at = ir
        .find("call i64 @js_object_alloc_class_inline_keys")
        .unwrap_or_else(|| panic!("the allocation must be emitted:\n{ir}"));
    let declare_at = ir[alloc_at..]
        .find(DECLARE_CALL)
        .map(|i| alloc_at + i)
        .unwrap_or_else(|| panic!("the layout declaration must be emitted:\n{ir}"));
    let ctor_call = ir[alloc_at..]
        .find("_Node_constructor(")
        .map(|i| alloc_at + i)
        .unwrap_or_else(|| panic!("the constructor call must be emitted:\n{ir}"));
    assert!(
        alloc_at < declare_at && declare_at < ctor_call,
        "expected alloc -> declare -> constructor; got alloc@{alloc_at} \
         declare@{declare_at} ctor@{ctor_call}:\n{ir}"
    );
}

/// The declaration must name the freshly allocated instance and carry the
/// class's raw-f64 mask with a NULL pointer mask — the gate admits only
/// all-scalar shapes, and a non-null pointer mask here would mean the
/// collector is being handed slots that still hold the allocator's fill.
#[test]
fn the_declaration_carries_a_raw_f64_mask_and_no_pointer_mask() {
    let ir = compile_ir(&declarable_module());
    let line = ir
        .lines()
        .find(|l| l.contains(DECLARE_CALL))
        .unwrap_or_else(|| panic!("no declaration in:\n{ir}"));
    assert!(
        line.contains("@perry_typed_shape_raw_f64_mask_"),
        "the raw-f64 mask must be passed: {line}"
    );
    assert!(
        line.contains("ptr null, i32 0"),
        "the pointer mask must be null/empty for a declarable class: {line}"
    );
}

/// Negative: a `number` field the prologue does not assign could be READ before
/// its first write, and would then see `undefined`'s NaN-box bits as a double.
#[test]
fn a_number_field_outside_the_prologue_refuses_the_declaration() {
    let ir = compile_ir(&module_with_new(
        class(
            "Partial",
            vec![field("v", Type::Number), field("w", Type::Number)],
            Some(ctor(
                // `w` is declared `number` but never assigned in the prologue.
                vec![param(10, "v", Type::Number), param(11, "w", Type::Number)],
                vec![assign_field_from_param("v", 10)],
            )),
        ),
        2,
    ));
    assert!(
        !ir.contains(DECLARE_CALL),
        "every raw-f64 field must be prologue-assigned, not just some:\n{ir}"
    );
}

/// P1 (#5094): a pointer field DOES get the declaration, and it carries a real
/// pointer mask.
///
/// This is the case #7510 deliberately excluded, and the exclusion cost the
/// class every store in its constructor: the post-constructor install arrives
/// after all of them, so each one missed its intact-bit guard and fell to
/// `js_object_set_field_by_name`. Obligation 2 is now discharged rather than
/// avoided — both `new` allocation paths pre-fill every slot with
/// `TAG_UNDEFINED`, which the tracer rejects at its tag check.
#[test]
fn a_pointer_field_gets_the_declaration_with_a_pointer_mask() {
    let ir = compile_ir(&module_with_new(
        class(
            "WithPointer",
            vec![field("v", Type::Number), field("name", Type::String)],
            Some(ctor(
                vec![
                    param(10, "v", Type::Number),
                    param(11, "name", Type::String),
                ],
                vec![
                    assign_field_from_param("v", 10),
                    assign_field_from_param("name", 11),
                ],
            )),
        ),
        2,
    ));
    let line = ir
        .lines()
        .find(|l| l.contains(DECLARE_CALL))
        .unwrap_or_else(|| panic!("a pointer-bearing class must declare:\n{ir}"));
    assert!(
        line.contains("@perry_typed_shape_raw_f64_mask_"),
        "the raw-f64 mask must still be passed: {line}"
    );
    assert!(
        line.contains("@perry_typed_shape_mask_"),
        "a pointer-bearing class must pass a non-null pointer mask: {line}"
    );
    assert!(
        !ir.contains(INIT_CALL),
        "the declaration still replaces the post-constructor install:\n{ir}"
    );
}

/// An untyped field lands on `Any`, which is pointer-bearing — so it takes the
/// same route the `string` field above does. This is what puts the synthesized
/// anon-shape classes behind object literals on the at-allocation declaration
/// (their inferred field types are all `Any`).
#[test]
fn an_untyped_field_gets_the_declaration() {
    let ir = compile_ir(&module_with_new(
        class(
            "Untyped",
            vec![field("v", Type::Number), field("other", Type::Any)],
            Some(ctor(
                vec![param(10, "v", Type::Number), param(11, "other", Type::Any)],
                vec![
                    assign_field_from_param("v", 10),
                    assign_field_from_param("other", 11),
                ],
            )),
        ),
        2,
    ));
    assert!(
        ir.contains(DECLARE_CALL),
        "`Any` is pointer-bearing, which is now a reason TO declare:\n{ir}"
    );
}

/// Negative: with neither a raw-f64 nor a pointer-bearing field, both masks are
/// empty — the declaration would install the state `layout_init_pointer_free`
/// already set and unlock nothing, so the extra call would be pure cost.
#[test]
fn a_class_with_no_number_field_refuses_the_declaration() {
    let ir = compile_ir(&module_with_new(
        class(
            "Flags",
            vec![field("a", Type::Boolean), field("b", Type::Boolean)],
            Some(ctor(
                vec![param(10, "a", Type::Boolean), param(11, "b", Type::Boolean)],
                vec![
                    assign_field_from_param("a", 10),
                    assign_field_from_param("b", 11),
                ],
            )),
        ),
        2,
    ));
    assert!(!ir.contains(DECLARE_CALL), "nothing to declare:\n{ir}");
}

/// Negative: a class with no constructor at all has no prologue, so nothing is
/// proven about when its fields are written.
#[test]
fn a_class_with_no_constructor_refuses_the_declaration() {
    let ir = compile_ir(&module_with_new(
        class("NoCtor", vec![field("v", Type::Number)], None),
        0,
    ));
    assert!(!ir.contains(DECLARE_CALL), "no prologue, no proof:\n{ir}");
}
