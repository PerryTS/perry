use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::{FunctionType, Type};
use perry_hir::{
    ArgumentsObjectMeta, BinaryOp, Class, ClassField, Expr, Function, Module, ModuleInitKind,
    Param, Stmt,
};

/// Serializes env-mutating tests so a concurrent test never observes a
/// half-applied variable. Mirrors the guard in `typed_shape_descriptors.rs`.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquires [`ENV_LOCK`] tolerating a poisoned mutex (#7490).
///
/// A test that panics while holding the lock poisons it during unwind; every
/// later `lock().unwrap()` in the binary then dies with `PoisonError` even
/// though its own subject is healthy. That is exactly what #7490 observed:
/// one genuine assertion failure cascaded into three PoisonError failures
/// under `--test-threads=1`, and under default parallelism the *victim set
/// wobbled* with scheduling — which read as order-dependent codegen state
/// when it was only lock poisoning.
///
/// Recovering the guard is sound here because the protected state is already
/// consistent at poison time: each test declares its `EnvVarGuard` *after*
/// the lock guard, so during unwind the env var is restored (guard `Drop`)
/// before the mutex is released. One test's failure must fail that test
/// alone.
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Sabotage test for [`env_lock`] (#7490): a test that panics while holding
/// the lock must fail *itself* and nothing else.
///
/// A gate has to assert its subject was live, not merely that nothing threw —
/// so this plants the exact failure shape #7490 cascaded from (an unwind out
/// of a lock-holding test), proves it really did poison the mutex, and then
/// demands the accessor still hands out a usable guard. It fails against the
/// pre-fix `ENV_LOCK.lock().unwrap()`, which is what makes a green run here
/// evidence rather than decoration.
///
/// Running first (`e` sorts ahead of `f`/`t`) also means every other test in
/// this binary executes under a genuinely poisoned lock, so the tolerance is
/// exercised for the whole suite rather than in one isolated case.
#[test]
fn env_lock_is_poison_tolerant_so_one_failure_cannot_cascade() {
    let sabotage = std::panic::catch_unwind(|| {
        let _guard = env_lock();
        panic!("#7490 sabotage: unwinding while holding ENV_LOCK");
    });
    assert!(sabotage.is_err(), "the sabotage panic should have unwound");
    assert!(
        ENV_LOCK.is_poisoned(),
        "unwinding out of a lock-holding test should poison ENV_LOCK — if it \
         no longer does, this test is no longer exercising its subject"
    );
    // The invariant under test: a later test still gets the lock.
    let _lock = env_lock();
}

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
        alloc_width_hint: 0,
        specialized_from: None,
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

    // Scope the ordering check to `main`'s body, and check every return site.
    // `main` has more than one `ret` (the host-return early exit returns
    // `i32 0`, the event-loop exit returns the pending exit code), and later
    // functions in the module carry their own returns. A positional
    // `rfind(dump) < rfind("ret i32 0")` over the whole module text therefore
    // compares two unrelated sites and proves nothing about the ordering.
    let body = entry_fn_body(&ir);
    let mut returns = 0;
    let mut prev = "";
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("ret ") {
            returns += 1;
            assert_eq!(
                prev, "call void @js_typed_feedback_maybe_dump_trace()",
                "every `main` return must dump the typed-feedback trace first, \
                 else a PERRY_TYPED_FEEDBACK trace is truncated at exit; \
                 found `{trimmed}` preceded by `{prev}`"
            );
        }
        prev = trimmed;
    }
    assert!(returns > 0, "expected at least one return in `main`");
}

/// Body of the entry `main` function, without its `define`/`}` lines.
///
/// The define line may carry function attributes after the parens — since
/// native roots became the default lowering (#7370) every generated function
/// is tagged `"frame-pointer"="non-leaf"`, and a stack-map-requesting `main`
/// would add `gc "statepoint-example"`. Match the exact signature, then cut
/// the body at that line's opening brace instead of demanding an
/// attribute-free header (which failed the whole test on unrelated attribute
/// changes, #7490).
fn entry_fn_body(ir: &str) -> &str {
    let sig = "define i32 @main()";
    let sig_start = ir
        .find(sig)
        .expect("entry module should define `i32 @main()`");
    let line_end = sig_start
        + ir[sig_start..]
            .find('\n')
            .expect("`main`'s define line should be newline-terminated");
    let header = &ir[sig_start..line_end];
    assert!(
        header.ends_with('{'),
        "`main`'s define line should end with its opening brace, got `{header}`"
    );
    let rest = &ir[line_end + 1..];
    let end = rest
        .find("\n}\n")
        .expect("`main` should be terminated by a closing brace");
    &rest[..end]
}

#[test]
fn typed_feedback_instruments_property_and_method_boundaries() {
    // Typed-feedback site *registration* is opt-in (emitted only when
    // PERRY_TYPED_FEEDBACK / _TRACE is set); this test exercises the enabled
    // path. Serialize on ENV_LOCK and restore the var on drop so concurrent or
    // later tests in this binary never observe the changed environment.
    let _lock = env_lock();
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
                    byte_offset: 0,
                    object: Box::new(Expr::LocalGet(1)),
                    property: "run".to_string(),
                }),
                args: vec![Expr::Number(2.0)],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                byte_offset: 0,
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
    assert!(ir.contains("call void @js_typed_feedback_observe_property_get"));
}

/// The negative twin of the test above, and the whole of #7480 step 4's second
/// half: a DEFAULT build emits none of the pure-recording helpers.
///
/// Every one of them early-returns unless the runtime env is set, so in a
/// default build each was a cross-crate call that answered "no" — 22.3% of
/// `churn_read_big.ts` between `observe_property` and `record_guard_pass`. The
/// registration call has been compile-gated on the same env since #5093's
/// follow-up; this extends that gate to the recording it registers for.
///
/// Asserted as a census over the whole emitted module rather than one site,
/// because the point is that NO path emits them — the generic-get diamond, the
/// array-push fallback, the index realloc arm, the method-override fallback and
/// the closure-call fallback all route through the same helper.
#[test]
fn a_default_build_emits_no_typed_feedback_recording_calls() {
    let _lock = env_lock();
    let _trace = EnvVarGuard::set("PERRY_TYPED_FEEDBACK_TRACE", None);
    let _env = EnvVarGuard::set("PERRY_TYPED_FEEDBACK", None);
    let ir = ir_for(module(
        "typed_feedback_default_build.ts",
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
                    byte_offset: 0,
                    object: Box::new(Expr::LocalGet(1)),
                    property: "run".to_string(),
                }),
                args: vec![Expr::Number(2.0)],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
            })),
        ],
    ));

    // The `declare` lines stay (they are emitted for the whole runtime surface
    // and LLVM drops the unused ones); what must be gone is every CALL.
    for helper in [
        "js_typed_feedback_observe_property_get",
        "js_typed_feedback_observe_property_set",
        "js_typed_feedback_record_guard_pass",
        "js_typed_feedback_record_guard_fail",
        "js_typed_feedback_record_fallback_call",
        "js_typed_feedback_register_site",
    ] {
        let call = format!("call void @{helper}");
        assert!(
            !ir.contains(&call),
            "a default build must not call `{helper}`; it early-returns at \
             runtime, so the call is pure overhead"
        );
    }

    // ANTI-VACUITY. Every assertion above is a negative, so they all pass on an
    // empty string. The property boundaries themselves must still be here —
    // this test proves the RECORDING is gone, not the program.
    assert!(
        ir.contains("js_object_get_field_by_name_f64")
            || ir.contains("js_object_get_field_ic_miss"),
        "the property reads themselves must still be lowered; emitted:\n{ir}"
    );
    // And the helpers that DECIDE something, rather than merely counting, are
    // not gated: this is the line between the two, asserted rather than
    // described.
    assert!(
        ir.contains("js_typed_feedback_object_set_field_by_name_fast"),
        "dispatching feedback wrappers must still be emitted in a default build"
    );
}

#[test]
fn typed_feedback_guards_direct_class_field_specialization() {
    // Serialize against the lever-B test (#5334), which sets the process-global
    // PERRY_FULL_OUTLINE_IC in this same test binary; pin it off so this test
    // always observes the inline diamond.
    let _lock = env_lock();
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
                    byte_offset: 0,
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
    // #8033: `receiver_class_name` now consults `stable_local_type_proof`
    // (runtime evidence), which a bare parameter lacks. The numeric-specific
    // `class_field_get_number.*` path is therefore not selected; the generic
    // `class_field_get.*` diamond is emitted instead, with its own fast/fallback
    // arms and the typed-feedback guard. The guard assertions above (lines
    // 493-499) validate the test's core purpose.
    assert!(ir.contains("class_field_get.fast"));
    assert!(ir.contains("class_field_get.fallback"));
    assert!(ir.contains("store double"));
    assert!(!ir.contains("call void @js_gc_note_slot_layout"));
    // #5334 lever A: the SET fallback arm collapses to one outlined call; the
    // by-name SET it replaced is no longer emitted at the set site.
    assert!(ir.contains("call void @js_class_field_set_fallback"));
    assert!(!ir.contains("call void @js_object_set_field_by_name"));
    // #7480 step 4: `record_fallback_call` used to be asserted present here
    // (from the class-field-GET fallback block, not the SET site — the SET copy
    // was folded into js_class_field_set_fallback by #5334). It is now emitted
    // only in a typed-feedback build, like the registration call beside it.
    // The fallback ARM itself is unchanged and is asserted above/below.
    assert!(!ir.contains("call void @js_typed_feedback_record_fallback_call"));
    assert!(ir.contains("call double @js_object_get_field_by_name_f64"));
}

/// Body of the first rendered block whose label starts with `label_prefix`,
/// or `None` if no such block exists.
///
/// Block labels carry per-function numeric suffixes (`.fast.6`, `.merge.8`),
/// so callers pass the stable prefix. A block header is a line-initial
/// `label:`; instruction lines that merely mention the label (branches, phis)
/// are indented, and are skipped. The body runs from the end of the label
/// line to the blank line separating it from the next block (or to the
/// function's closing brace).
#[allow(dead_code)]
fn block_body<'a>(ir: &'a str, label_prefix: &str) -> Option<&'a str> {
    let needle = format!("\n{label_prefix}");
    let mut from = 0;
    while let Some(rel) = ir[from..].find(&needle) {
        let label_start = from + rel + 1;
        let line_end = label_start + ir[label_start..].find('\n')?;
        if ir[label_start..line_end].ends_with(':') {
            let rest = &ir[line_end + 1..];
            // A function-final block is terminated by the closing brace, an
            // interior one by the blank separator line — whichever comes
            // first bounds this block.
            let end = match (rest.find("\n\n"), rest.find("\n}")) {
                (Some(a), Some(b)) => a.min(b),
                (a, b) => a.or(b).unwrap_or(rest.len()),
            };
            return Some(&rest[..end]);
        }
        from = line_end;
    }
    None
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

    let _lock = env_lock();

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
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
            }))],
        )
    };

    let _lock = env_lock();

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

    let _lock = env_lock();

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

    let _lock = env_lock();
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
    let _lock = env_lock();
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
    let _lock = env_lock();
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
                byte_offset: 0,
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
    // Class field initialization follows DefineField semantics. Since #8653
    // this fixture takes the guarded fast path rather than an unconditional
    // `js_class_field_add`: the field is a known slot with no accessor
    // anywhere on the chain and the chain's constructor cannot replace
    // `this`, so the store is specialized behind a shape guard. DefineField
    // semantics are preserved on the fallback arm, which is what keeps an
    // inherited setter from running.
    assert!(ir.contains("js_typed_feedback_class_field_set_guard"));
    assert!(ir.contains("class_field_set.fast"));
    assert!(ir.contains("class_field_set.fallback"));
    assert!(ir.contains("call void @js_class_field_set_fallback"));
    // The unconditional helper must NOT be called for this shape -- that was
    // the #8648 regression (3.11x on `shapes`) that #8653 reverted.
    assert!(!ir.contains("call double @js_class_field_add"));
    assert!(ir.contains("call double @js_native_call_method"));
}

#[test]
fn synthetic_arguments_only_method_uses_shape_guarded_direct_call() {
    let mut counter = class(104, "Counter", Vec::new());
    counter.methods.push(Function {
        id: 8,
        name: "count".to_string(),
        type_params: Vec::new(),
        params: vec![
            param(2, "value", Type::Any),
            Param {
                id: 3,
                name: "arguments".to_string(),
                ty: Type::Any,
                default: None,
                decorators: Vec::new(),
                is_rest: true,
                arguments_object: Some(ArgumentsObjectMeta {
                    strict: false,
                    simple_parameters: true,
                    mapped_parameter_ids: Vec::new(),
                    restricted_callee: false,
                }),
            },
        ],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(3)),
            property: "length".to_string(),
            byte_offset: 0,
        }))],
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
        "synthetic_arguments_method_guard.ts",
        vec![counter],
        vec![param(1, "counter", Type::Named("Counter".to_string()))],
        Type::Number,
        vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "count".to_string(),
                byte_offset: 0,
            }),
            args: vec![Expr::Number(7.0)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
    ));
    let probe_start = ir
        .find("define double @perry_fn_synthetic_arguments_method_guard_ts__probe")
        .expect("probe should be emitted");
    let probe_tail = &ir[probe_start..];
    let probe_end = probe_tail
        .find("\n}\n")
        .expect("probe should have a complete definition");
    let probe_ir = &probe_tail[..probe_end];

    assert!(
        probe_ir.contains("method_direct.inline_deref")
            && probe_ir.contains("method_direct.fast")
            && probe_ir.contains("method_direct.fallback"),
        "synthetic-arguments-only methods should use the exact-shape direct guard:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i64 @js_array_mark_arguments_object")
            && probe_ir.contains(
                "call double @perry_method_synthetic_arguments_method_guard_ts__Counter__count",
            ),
        "the direct arm must preserve the concrete method's synthesized-arguments ABI:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call double @js_native_call_method_by_id")
            && !probe_ir.contains("call double @js_object_get_own_field_or_undef"),
        "a guard miss should preserve dynamic override dispatch without an eager own-property scan:\n{probe_ir}"
    );
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
    // #7480 step 4: pure-recording feedback helpers are emitted only in a
    // typed-feedback build. The fallback arm is asserted by the two lines
    // above; this asserts the counter that rode along with it is gone.
    assert!(!ir.contains("call void @js_typed_feedback_record_fallback_call"));
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
    let _lock = env_lock();
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
                op: BinaryOp::BitAnd,
                left: Box::new(Expr::LocalGet(2)),
                right: Box::new(Expr::Integer(63)),
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
