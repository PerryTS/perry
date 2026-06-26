use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{BinaryOp, Class, ClassField, Expr, Function, Module, ModuleInitKind, Param, Stmt};
use perry_types::{FunctionType, Type};

/// Serializes env-mutating tests so a concurrent test never observes a
/// half-applied variable. Mirrors the guard in `typed_shape_descriptors.rs`.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Sets an env var for the duration of a test and restores the previous value
/// (or unsets it) on drop, so the mutation never leaks to other tests.
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

fn class(id: u32, name: &str, fields: Vec<ClassField>) -> Class {
    Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields,
        constructor: None,
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
    }
}

fn module(name: &str, params: Vec<Param>, return_type: Type, body: Vec<Stmt>) -> Module {
    module_with_classes(name, Vec::new(), params, return_type, body)
}

fn module_with_classes(
    name: &str,
    classes: Vec<Class>,
    params: Vec<Param>,
    return_type: Type,
    body: Vec<Stmt>,
) -> Module {
    Module {
        name: name.to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes,
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
            type_params: Vec::new(),
            params,
            return_type,
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
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

fn ir_for(module: Module) -> String {
    String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap()
}

fn entry_ir_for(module: Module) -> String {
    let mut opts = empty_opts();
    opts.is_entry_module = true;
    String::from_utf8(compile_module(&module, opts).unwrap()).unwrap()
}

#[test]
fn typed_feedback_trace_dump_runs_before_entry_return() {
    let ir = entry_ir_for(module(
        "typed_feedback_epilogue.ts",
        Vec::new(),
        Type::Void,
        Vec::new(),
    ));

    assert!(ir.contains("declare void @js_typed_feedback_maybe_dump_trace()"));
    let dump_pos = ir
        .rfind("call void @js_typed_feedback_maybe_dump_trace()")
        .expect("entry should call typed-feedback trace dump");
    let ret_pos = ir.rfind("ret i32 0").expect("entry should return i32 0");
    assert!(dump_pos < ret_pos);
}

#[test]
fn typed_feedback_instruments_property_and_method_boundaries() {
    // Typed-feedback site *registration* is opt-in (emitted only when
    // PERRY_TYPED_FEEDBACK / _TRACE is set); this test exercises the enabled
    // path. Serialize on ENV_LOCK and restore the var on drop so concurrent or
    // later tests in this binary never observe the changed environment.
    let _lock = ENV_LOCK.lock().unwrap();
    let _env = EnvVarGuard::set("PERRY_TYPED_FEEDBACK", Some("1"));
    let ir = ir_for(module(
        "typed_feedback_property.ts",
        vec![param(1, "obj", Type::Any)],
        Type::Any,
        vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
                value: Box::new(Expr::Number(1.0)),
            }),
            Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: "run".to_string(),
                }),
                args: vec![Expr::Number(2.0)],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
            })),
        ],
    ));

    assert!(ir.contains("@perry_typed_feedback_"));
    assert!(ir.contains("call void @js_typed_feedback_register_site"));
    assert!(ir.contains("object_set_by_name_guard"));
    assert!(ir.contains("object_get_by_name_guard"));
    assert!(ir.contains("method_call_guard"));
    assert!(ir.contains("js_typed_feedback_object_set_field_by_name_fast"));
    assert!(ir.contains("js_object_set_field_by_name"));
    assert!(ir.contains("js_object_get_field_by_name_f64"));
    assert!(ir.contains("call double @js_typed_feedback_native_call_method"));
    assert!(ir.contains("call void @js_typed_feedback_record_guard_pass"));
    assert!(ir.contains("call void @js_typed_feedback_record_guard_fail"));
    assert!(ir.contains("call void @js_typed_feedback_record_fallback_call"));
}

#[test]
fn typed_feedback_guards_direct_class_field_specialization() {
    // Serialize against the lever-B test (#5334), which sets the process-global
    // PERRY_FULL_OUTLINE_IC in this same test binary; pin it off so this test
    // always observes the inline diamond.
    let _lock = ENV_LOCK.lock().unwrap();
    let _g = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", Some("0"));
    let point = class(101, "Point", vec![field("x", Type::Number)]);
    let ir = ir_for(module_with_classes(
        "typed_feedback_class_field.ts",
        vec![point],
        vec![param(1, "p", Type::Named("Point".to_string()))],
        Type::Number,
        vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
                value: Box::new(Expr::Number(7.0)),
            }),
            Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Sub,
                left: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: "x".to_string(),
                }),
                right: Box::new(Expr::Integer(1)),
            })),
        ],
    ));

    assert!(ir.contains("class_field_set_guard"));
    assert!(ir.contains("class_field_get_guard"));
    assert!(ir.contains("@perry_typed_shape_raw_f64_mask_"));
    assert!(ir.contains("js_typed_feedback_class_field_set_guard"));
    assert!(ir.contains("js_typed_feedback_class_field_get_guard"));
    assert!(ir.contains("class_field_set.fast"));
    assert!(ir.contains("class_field_set.fallback"));
    assert!(ir.contains("class_field_get.fast"));
    assert!(ir.contains("class_field_get.fallback"));
    assert!(ir.contains("store double"));
    assert!(!ir.contains("call void @js_gc_note_slot_layout"));
    // #5334 lever A: the SET fallback arm collapses to one outlined call; the
    // by-name SET it replaced is no longer emitted at the set site.
    assert!(ir.contains("call void @js_class_field_set_fallback"));
    assert!(!ir.contains("call void @js_object_set_field_by_name"));
    // `record_fallback_call` is still present — but from the class-field-GET
    // fallback block below, not the SET site (the SET copy is now folded into
    // js_class_field_set_fallback).
    assert!(ir.contains("call void @js_typed_feedback_record_fallback_call"));
    assert!(ir.contains("call double @js_object_get_field_by_name_f64"));
    assert!(
        ir.contains("call double @js_number_coerce"),
        "class-field raw fallback must be coerced at numeric consumers:\n{ir}"
    );
}

#[test]
fn full_outline_ic_collapses_class_field_set_to_single_call() {
    // #5334 lever B: when full-outline is enabled (oversized module, or forced
    // via env), the entire class-field-SET diamond collapses to a single
    // `js_class_field_set_ic` call — no guard call, no fast/fallback blocks.
    let build = || {
        let point = class(101, "Point", vec![field("x", Type::Number)]);
        module_with_classes(
            "full_outline_field.ts",
            vec![point],
            vec![param(1, "p", Type::Named("Point".to_string()))],
            Type::Number,
            vec![
                Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: "x".to_string(),
                    value: Box::new(Expr::Number(7.0)),
                }),
                Stmt::Return(Some(Expr::Number(0.0))),
            ],
        )
    };

    let _lock = ENV_LOCK.lock().unwrap();

    // Forced ON: one outlined call, no inline diamond.
    {
        let _g = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", Some("1"));
        let ir = ir_for(build());
        assert!(ir.contains("call void @js_class_field_set_ic"));
        assert!(!ir.contains("class_field_set.fast"));
        assert!(!ir.contains("class_field_set.fallback"));
        assert!(!ir.contains("call i32 @js_typed_feedback_class_field_set_guard"));
    }

    // Forced OFF (the default for normal-sized modules): the inline diamond,
    // and no full-outline call.
    {
        let _g = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", Some("0"));
        let ir = ir_for(build());
        assert!(!ir.contains("call void @js_class_field_set_ic"));
        assert!(ir.contains("class_field_set.fast"));
        assert!(ir.contains("js_typed_feedback_class_field_set_guard"));
    }
}

#[test]
fn full_outline_ic_collapses_class_field_get_to_single_call() {
    // #5391 path 2: full-outline collapses the class-field-GET diamond to a
    // single `js_class_field_get_ic` call returning the field value.
    let build = || {
        let point = class(101, "Point", vec![field("x", Type::Number)]);
        module_with_classes(
            "full_outline_get.ts",
            vec![point],
            vec![param(1, "p", Type::Named("Point".to_string()))],
            Type::Number,
            vec![Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
            }))],
        )
    };

    let _lock = ENV_LOCK.lock().unwrap();

    // Forced ON: one outlined call, no inline get diamond.
    {
        let _g = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", Some("1"));
        let ir = ir_for(build());
        assert!(ir.contains("call double @js_class_field_get_ic"));
        assert!(!ir.contains("class_field_get.fast"));
        assert!(!ir.contains("class_field_get.fallback"));
        assert!(!ir.contains("call i32 @js_typed_feedback_class_field_get_guard"));
    }

    // Forced OFF: the inline diamond, no full-outline call.
    {
        let _g = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", Some("0"));
        let ir = ir_for(build());
        assert!(!ir.contains("call double @js_class_field_get_ic"));
        assert!(ir.contains("class_field_get.fast"));
        assert!(ir.contains("js_typed_feedback_class_field_get_guard"));
    }
}

#[test]
fn full_outline_array_literal_uses_builder_call() {
    // #5391: full-outline replaces the inline array-literal construction
    // (bump-alloc diamond + per-element store/note/barrier) with one
    // `js_array_from_values` call over a stack buffer.
    // A param-based array (not all-const) so it reaches `lower_array_literal`
    // rather than const-folding to a flat rodata global.
    let build = || {
        module(
            "outline_arr.ts",
            vec![param(1, "x", Type::Number)],
            Type::Any,
            vec![Stmt::Return(Some(Expr::Array(vec![
                Expr::LocalGet(1),
                Expr::Number(2.0),
                Expr::LocalGet(1),
            ])))],
        )
    };

    let _lock = ENV_LOCK.lock().unwrap();

    {
        let _g = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", Some("1"));
        let ir = ir_for(build());
        assert!(ir.contains("call i64 @js_array_from_values"));
        assert!(!ir.contains("arrlit.fast"));
        // the inline bump-alloc CALL (not its always-present declare) is gone
        assert!(!ir.contains("call ptr @js_inline_arena_slow_alloc"));
    }
    {
        // OFF: the inline construction path (whatever it is for this literal) —
        // crucially NOT the outlined builder.
        let _g = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", Some("0"));
        let ir = ir_for(build());
        assert!(!ir.contains("call i64 @js_array_from_values"));
    }
}

#[test]
fn full_outline_ic_auto_gate_counts_class_methods() {
    // #5334 lever B: the auto size-gate counts class CALLABLES (methods,
    // accessors, ctor), not just top-level `hir.functions`. A class-heavy module
    // (the minified-bundle pathology) must trigger even though it has only one
    // top-level function — class methods/closures don't live in `hir.functions`.
    let mut big = class(150, "Big", vec![field("x", Type::Number)]);
    for i in 0..6u32 {
        big.methods.push(Function {
            id: 200 + i,
            name: format!("m{i}"),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Number,
            body: vec![Stmt::Return(Some(Expr::Number(0.0)))],
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });
    }
    let module = module_with_classes(
        "auto_gate.ts",
        vec![big],
        vec![param(1, "p", Type::Named("Big".to_string()))],
        Type::Number,
        vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
                value: Box::new(Expr::Number(7.0)),
            }),
            Stmt::Return(Some(Expr::Number(0.0))),
        ],
    );

    let _lock = ENV_LOCK.lock().unwrap();
    // Auto path (override unset): callable count = 1 probe fn + 6 methods = 7,
    // which clears MIN_FUNCS=5 even though `hir.functions.len()` is just 1. The
    // pre-fix function-only count (1) would have stayed under the threshold.
    let _ic = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", None);
    let _min = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC_MIN_FUNCS", Some("5"));
    let ir = ir_for(module);
    assert!(ir.contains("call void @js_class_field_set_ic"));
    assert!(!ir.contains("class_field_set.fast"));
}

#[test]
fn class_field_set_elides_write_barrier_for_nonpointer_value() {
    // #5334 lever D: storing a value that is a non-pointer by construction
    // into a BOXED class field (a String slot — only Number is raw-f64) skips
    // the generational write barrier, since the store creates no parent→child
    // heap reference. The layout note still fires (it tracks the slot's
    // pointer-ness). A value that may be a heap pointer keeps the barrier.
    //
    // Serialize against the lever-B full-outline test (#5334) and pin
    // PERRY_FULL_OUTLINE_IC off, so this test always observes the inline diamond
    // (`class_field_set.fast`) rather than the outlined call.
    let _lock = ENV_LOCK.lock().unwrap();
    let _g = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", Some("0"));
    let build = |val: Expr| {
        let c = class(140, "Bx", vec![field("s", Type::String)]);
        module_with_classes(
            "lever_d_field_barrier.ts",
            vec![c],
            vec![
                param(1, "o", Type::Named("Bx".to_string())),
                param(2, "p", Type::String),
            ],
            Type::Number,
            vec![
                Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: "s".to_string(),
                    value: Box::new(val),
                }),
                Stmt::Return(Some(Expr::Number(0.0))),
            ],
        )
    };

    // A numeric-by-construction value takes the boxed class-field fast store
    // but needs no write barrier.
    let ir_num = ir_for(build(Expr::Number(7.0)));
    assert!(ir_num.contains("class_field_set.fast"));
    assert!(!ir_num.contains("call void @js_write_barrier_slot"));

    // A definite heap pointer (string literal) keeps the slot barrier.
    let ir_ptr = ir_for(build(Expr::String("hi".to_string())));
    assert!(ir_ptr.contains("call void @js_write_barrier_slot"));
}

#[test]
fn typed_feedback_guards_direct_class_method_specialization() {
    // Serialize against the lever-B test (#5334) and pin full-outline off so the
    // class's synthesized field-set keeps its inline fallback (asserted below).
    let _lock = ENV_LOCK.lock().unwrap();
    let _g = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", Some("0"));
    let mut point = class(103, "Point", vec![field("x", Type::Number)]);
    point.methods.push(Function {
        id: 7,
        name: "inc".to_string(),
        type_params: Vec::new(),
        params: vec![param(2, "n", Type::Number)],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::LocalGet(2)))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let ir = ir_for(module_with_classes(
        "typed_feedback_class_method.ts",
        vec![point],
        vec![param(1, "p", Type::Named("Point".to_string()))],
        Type::Number,
        vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "inc".to_string(),
            }),
            args: vec![Expr::Number(5.0)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
    ));

    assert!(ir.contains("method_direct_call_guard"));
    assert!(ir.contains("js_typed_feedback_method_direct_call_guard"));
    assert!(ir.contains("method_direct.fast"));
    assert!(ir.contains("method_direct.fallback"));
    // #5334 lever A: this class has a field `x` whose synthesized field-set
    // routes its guard-miss arm through the outlined fallback. (The
    // method-direct fallback only records when its site_id is Some, which it
    // isn't here — the old `record_fallback_call` assertion was incidentally
    // satisfied by the field-set fallback that is now folded into this call.)
    assert!(ir.contains("call void @js_class_field_set_fallback"));
    assert!(ir.contains("call double @js_native_call_method"));
}

#[test]
fn typed_feedback_guards_direct_closure_call_specialization() {
    let closure_ty = Type::Function(FunctionType {
        params: vec![("x".to_string(), Type::Number, false)],
        return_type: Box::new(Type::Number),
        is_async: false,
        is_generator: false,
    });
    let ir = ir_for(module(
        "typed_feedback_closure_call.ts",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 2,
                name: "cb".to_string(),
                ty: closure_ty,
                mutable: false,
                init: Some(Expr::Closure {
                    func_id: 44,
                    params: vec![param(3, "x", Type::Number)],
                    return_type: Type::Number,
                    body: vec![Stmt::Return(Some(Expr::LocalGet(3)))],
                    captures: Vec::new(),
                    mutable_captures: Vec::new(),
                    captures_this: false,
                    captures_new_target: false,
                    enclosing_class: None,
                    is_arrow: false,
                    is_async: false,
                    is_generator: false,
                    is_strict: false,
                }),
            },
            Stmt::Return(Some(Expr::Call {
                callee: Box::new(Expr::LocalGet(2)),
                args: vec![Expr::Number(9.0)],
                type_args: Vec::new(),
                byte_offset: 0,
            })),
        ],
    ));

    assert!(ir.contains("closure_direct_call_guard"));
    assert!(ir.contains("js_typed_feedback_closure_direct_call_guard"));
    assert!(ir.contains("closure_direct.fast"));
    assert!(ir.contains("closure_direct.fallback"));
    assert!(ir.contains("call double @perry_closure_"));
    assert!(ir.contains("call double @js_closure_call1"));
    assert!(ir.contains("call void @js_typed_feedback_record_fallback_call"));
}

#[test]
fn typed_feedback_guards_array_index_specialization() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let ir = ir_for(module(
        "typed_feedback_array.ts",
        vec![param(1, "xs", array_ty)],
        Type::Number,
        vec![
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Number(0.0)),
                value: Box::new(Expr::Number(7.0)),
            }),
            Stmt::Return(Some(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Number(0.0)),
            })),
        ],
    ));

    assert!(ir.contains("numeric_array_index_set_guard"));
    assert!(ir.contains("numeric_array_index_get_guard"));
    assert!(ir.contains("js_typed_feedback_numeric_array_index_set_guard"));
    assert!(ir.contains("js_typed_feedback_array_index_set_fallback_boxed"));
    assert!(ir.contains("js_typed_feedback_numeric_array_index_get_guard"));
    assert!(ir.contains("js_typed_feedback_array_index_get_fallback_boxed"));
    assert!(ir.contains("idxset.inbounds"));
    assert!(ir.contains("store double"));
    assert!(!ir.contains("call i32 @js_array_numeric_set_f64_unboxed"));
    assert!(!ir.contains("call double @js_array_numeric_get_f64_unboxed"));
}

#[test]
fn typed_feedback_guards_numeric_array_push_specialization() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let ir = ir_for(module(
        "typed_feedback_array_push.ts",
        vec![],
        array_ty.clone(),
        vec![
            Stmt::Let {
                id: 1,
                name: "xs".to_string(),
                ty: array_ty,
                mutable: true,
                init: Some(Expr::Array(Vec::new())),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: 1,
                value: Box::new(Expr::Number(7.0)),
            }),
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    ));

    assert!(ir.contains("numeric_array_push_guard"));
    assert!(ir.contains("js_typed_feedback_numeric_array_push_guard"));
    assert!(ir.contains("js_array_numeric_push_f64_unboxed"));
    assert!(ir.contains("js_typed_feedback_record_fallback_call"));
    assert!(ir.contains("call i64 @js_array_push_f64"));
}

#[test]
fn typed_feedback_marks_numeric_array_literals() {
    // Serialize against the array-literal full-outline test and pin it off, so
    // this test always observes the inline numeric-array construction
    // (js_array_mark_numeric_f64_layout) rather than the outlined builder.
    let _lock = ENV_LOCK.lock().unwrap();
    let _g = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", Some("0"));
    let numeric_ir = ir_for(module(
        "typed_feedback_numeric_array_literal.ts",
        Vec::new(),
        Type::Any,
        vec![Stmt::Return(Some(Expr::Array(vec![
            Expr::Number(1.0),
            Expr::Integer(2),
            Expr::Binary {
                op: perry_hir::BinaryOp::Mul,
                left: Box::new(Expr::Number(3.0)),
                right: Box::new(Expr::Number(4.0)),
            },
        ])))],
    ));

    assert!(numeric_ir.contains("call i32 @js_array_mark_numeric_f64_layout"));

    let mixed_ir = ir_for(module(
        "typed_feedback_mixed_array_literal.ts",
        Vec::new(),
        Type::Any,
        vec![Stmt::Return(Some(Expr::Array(vec![
            Expr::Number(1.0),
            Expr::String("x".to_string()),
        ])))],
    ));

    assert!(!mixed_ir.contains("call i32 @js_array_mark_numeric_f64_layout"));
}

#[test]
fn typed_feedback_inline_array_writes_note_numeric_downgrade() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let ir = ir_for(module(
        "typed_feedback_array_numeric_downgrade.ts",
        Vec::new(),
        Type::Any,
        vec![
            Stmt::Let {
                id: 2,
                name: "xs".to_string(),
                ty: array_ty,
                mutable: true,
                init: Some(Expr::Array(vec![Expr::Number(1.0)])),
            },
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(2)),
                index: Box::new(Expr::Number(0.0)),
                value: Box::new(Expr::String("not-number".to_string())),
            }),
            Stmt::Expr(Expr::ArrayPush {
                array_id: 2,
                value: Box::new(Expr::String("still-not-number".to_string())),
            }),
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
    ));

    assert!(ir.contains("call void @js_array_note_numeric_write"));
    assert!(ir.contains("plain_array_index_set_guard"));
    assert!(ir.contains("js_typed_feedback_plain_array_index_set_guard"));
    assert!(!ir.contains("call i32 @js_typed_feedback_numeric_array_index_set_guard"));
}

#[test]
fn typed_feedback_guards_computed_numeric_array_index_hot_path() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let ir = ir_for(module(
        "typed_feedback_computed_array.ts",
        vec![param(1, "xs", array_ty), param(2, "i", Type::Number)],
        Type::Number,
        vec![Stmt::Return(Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Binary {
                op: BinaryOp::Mod,
                left: Box::new(Expr::LocalGet(2)),
                right: Box::new(Expr::Integer(64)),
            }),
        }))],
    ));

    assert!(ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard"));
    assert!(ir.contains("call double @js_typed_feedback_array_index_get_fallback_boxed"));
    // The numeric fast path no longer calls `js_array_numeric_get_f64_unboxed`:
    // the guard already proved raw-f64 layout + in-bounds, so the slot is loaded
    // inline (a direct `load double` from the element address).
    assert!(!ir.contains("call double @js_array_numeric_get_f64_unboxed"));
    assert!(ir.contains("load double"));
}
