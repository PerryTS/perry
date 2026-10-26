use perry_codegen::{compile_module, AppMetadata, CompileOptions};

#[path = "native_proof_support/read_memo_contract.rs"]
mod read_memo_contract;
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
        static_shape_ids: Vec::new(),
        program_layout_ordinals: Default::default(),
        program_class_shape_ids: Default::default(),
        program_facts: Default::default(),
        program_layouts: Default::default(),
        gc_effects: Default::default(),
        target: None,
        is_entry_module: false,
        non_entry_module_prefixes: Vec::new(),
        thread_literal_module_prefixes: Vec::new(),
        import_function_prefixes: std::collections::HashMap::new(),
        import_function_ffi_aliases: std::collections::HashMap::new(),
        import_function_origin_names: std::collections::HashMap::new(),
        import_function_v8_specifiers: std::collections::HashMap::new(),
        import_function_node_submodule: std::collections::HashMap::new(),
        namespace_node_submodules: std::collections::HashMap::new(),
        namespace_v8_specifiers: std::collections::HashMap::new(),
        namespace_member_prefixes: std::collections::HashMap::new(),
        namespace_source_prefixes: Default::default(),
        namespace_member_origin_names: std::collections::HashMap::new(),
        emit_ir_only: true,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        namespace_imports: Vec::new(),
        namespace_member_nested: Vec::new(),
        cjs_interop_namespaces: Vec::new(),
        constructor_param_counts: Default::default(),
        imported_classes: Vec::new(),
        short_spread_method_candidates: std::sync::Arc::default(),
        program_class_accessor_names: Default::default(),
        object_literal_method_candidates: std::sync::Arc::default(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: Default::default(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_synthetic_arguments: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        disable_constfn_shapes: false,
        program_has_worker: false,
        program_has_thread_agents: false,
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
        fp_contract_mode: perry_codegen::FpContractMode::Off,
        app_metadata: AppMetadata::default(),
        namespace_entries: Vec::new(),
        dynamic_import_path_to_prefix: std::collections::HashMap::new(),
        nextjs_path_init_modules: Vec::new(),
        deferred_module_prefixes: Default::default(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        namespace_adopts_cjs_exports: false,
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
        origin: perry_hir::ClassFieldOrigin::Definition,
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

/// #9459: the same module with `probe` (and module init) STRICT.
///
/// `module_with_classes` hard-codes `is_strict: false`, which used to be
/// invisible: `Expr::PropertySet` carried no strictness and every store took one
/// lowering. It now takes the assignment's own `Throw` flag from
/// `FnCtx::is_strict_fn`, and only the STRICT store tail carries the
/// typed-feedback `PropertySet` site -- the sloppy tail is
/// `js_put_value_set(..., 0)`, so a rejected sloppy `[[Set]]` stays a silent
/// no-op. A test whose subject is that site has to ask for the lane it lives on,
/// the same expectation move #9458 made when `Module::init_is_strict` landed.
fn strict_module(name: &str, params: Vec<Param>, return_type: Type, body: Vec<Stmt>) -> Module {
    let mut module = module_with_classes(name, Vec::new(), params, return_type, body);
    module.init_is_strict = true;
    for function in &mut module.functions {
        function.is_strict = true;
    }
    module
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
        init_is_strict: false,
        init: Vec::new(),
        classic_for_lexical_bindings: std::collections::HashSet::new(),
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
        class_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        local_source_spans: std::collections::HashMap::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
        class_family_syntax: None,
        requested_modules: Vec::new(),
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
    // #9459: STRICT -- see `strict_module`. #9495: the strict and sloppy tails
    // are now ONE tail (`js_put_value_set` with the assignment's own `Throw`
    // flag), so the `object_set_by_name_guard` site asserted here is registered
    // in both modes; the sloppy twin below asserts the flag, not a different
    // lane.
    let ir = ir_for(strict_module(
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
    // #9495: the strict by-name store is the receiver-aware `[[Set]]`
    // (`js_put_value_set(..., 1)`), observed by the pure-recording
    // `js_typed_feedback_observe_property_set` in an emitting build -- not the
    // own-property `js_typed_feedback_object_set_field_by_name_fast` ->
    // `js_object_set_field_by_name` dispatcher, which skipped the prototype
    // walk. Match CALLS: every runtime entry is `declare`d in every module.
    assert!(
        ir.contains("call void @js_typed_feedback_observe_property_set("),
        "an emitting build observes the strict by-name store:\n{ir}"
    );
    assert_eq!(
        put_value_set_strict_flags(&ir),
        vec!["1"],
        "the strict store reaches `js_put_value_set` with Throw = 1:\n{ir}"
    );
    assert!(
        !ir.contains("call void @js_typed_feedback_object_set_field_by_name_fast("),
        "the own-property store dispatcher must not be CALLED on a strict \
         by-name store -- it skips the prototype walk (#9495):\n{ir}"
    );
    assert!(ir.contains("js_object_get_field_by_name_f64"));
    assert!(ir.contains("call double @js_typed_feedback_native_call_method"));
    // Property and method sites hold no guard of their own any more (the layout
    // probe is the whole of the first stage), so there is no guard outcome to
    // record: an emitting build REGISTERS each site and OBSERVES its operands,
    // and never calls the guard-outcome recorders.
    assert!(ir.contains("call void @js_typed_feedback_observe_property_get("));
    for gone in [
        "call void @js_typed_feedback_record_guard_pass(",
        "call void @js_typed_feedback_record_guard_fail(",
        "call void @js_typed_feedback_record_fallback_call(",
    ] {
        assert!(!ir.contains(gone), "{gone} has no guard to report:\n{ir}");
    }
}

/// The `i32` `Throw` flag of every `js_put_value_set` CALL in `ir`, in emission
/// order. Matches the call, never the `declare` line, and reads the flag off
/// the call's last operand so a test can pin WHICH mode reached the entry.
fn put_value_set_strict_flags(ir: &str) -> Vec<&str> {
    // The static-key store IC's one miss entry is `js_put_value_set` plus the
    // site's publication: `(double target, i64 key, double value, i32 strict,
    // ptr ways, ptr word)`. Its only `i32` argument is the Throw flag.
    ir.lines()
        .filter(|line| {
            line.contains("call double @js_put_value_set(")
                || line.contains("call double @js_put_value_set_packed_miss(")
        })
        .map(|line| {
            let args = line.rsplit_once(')').map(|(head, _)| head).unwrap_or(line);
            let flag = args
                .split("i32 ")
                .nth(1)
                .expect("a [[Set]] call carries its i32 Throw flag");
            flag.split(',').next().unwrap_or(flag).trim()
        })
        .collect()
}

/// #9459: the sloppy twin of the boundaries test above.
///
/// A sloppy `o.x = v` must not reach `js_object_set_field_by_name` -- it has no
/// `strict` parameter and rejects a non-writable slot by throwing, where sloppy
/// `PutValue` discards the rejection. It goes to `js_put_value_set(..., 0)`.
///
/// #9495: the strict tail is the same entry with `1`, so the two modes are now
/// distinguished by the flag alone; the typed-feedback `PropertySet` site is
/// registered in both (it describes the store, not its strictness), and the
/// old own-property dispatcher is called in neither.
///
/// The GET boundary is asserted too, and is deliberately unchanged: strictness
/// is a property of `PutValue`, not of `GetValue`. That assertion is also what
/// keeps this test from passing on an empty program.
#[test]
fn sloppy_property_set_uses_strictness_aware_put_value() {
    let _lock = env_lock();
    let _env = EnvVarGuard::set("PERRY_TYPED_FEEDBACK", Some("1"));
    let ir = ir_for(module(
        "typed_feedback_property_sloppy.ts",
        vec![param(1, "obj", Type::Any)],
        Type::Any,
        vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
                value: Box::new(Expr::Number(1.0)),
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
            })),
        ],
    ));

    // Match the CALL, not the symbol: every runtime entry is `declare`d in
    // every module, so `contains("js_put_value_set")` would pass on a module
    // that never stores and `contains("...set_field_by_name_fast")` would fail
    // on one that never does either.
    assert_eq!(
        put_value_set_strict_flags(&ir),
        vec!["0"],
        "a sloppy property store should reach the strictness-aware [[Set]] with \
         Throw = 0:\n{ir}"
    );
    assert!(
        !ir.contains("call void @js_typed_feedback_object_set_field_by_name_fast("),
        "the own-property store dispatcher must not be CALLED on a sloppy \
         store -- its underlying setter throws on a rejected write:\n{ir}"
    );
    assert!(
        ir.contains("object_set_by_name_guard")
            && ir.contains("call void @js_typed_feedback_observe_property_set("),
        "the typed-feedback SET site is registered and observed on the sloppy \
         tail too -- it is one tail since #9495:\n{ir}"
    );
    // ANTI-VACUITY: the reads are untouched by strictness and must still be
    // instrumented, so the negatives above are about the store lane and not
    // about an empty module.
    assert!(
        ir.contains("object_get_by_name_guard"),
        "the property READ boundary is strictness-independent and must remain:\n{ir}"
    );
    assert!(
        ir.contains("js_object_get_field_by_name_f64"),
        "the property read itself must still be lowered:\n{ir}"
    );
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
        ir.contains("call double @perry_probe_get(")
            && read_memo_contract::paired_memo_reads(&ir) == Some(1)
            && ir
                .matches(" = private global [5 x i64] zeroinitializer")
                .count()
                == 1,
        "the property reads themselves must still be lowered; emitted:\n{ir}"
    );
    // And the helpers that DECIDE something, rather than merely counting, are
    // not gated: this is the line between the two, asserted rather than
    // described. The property GET (layout probe, its complete `[[Get]]` miss and
    // the non-pointer receiver exit), the method call (the slow method call) and
    // the by-name store (the store probe and the receiver-aware `[[Set]]`) are
    // all emitted in a default build, as CALLS (every runtime entry is
    // `declare`d, so a bare symbol match would be satisfied by the declaration).
    assert!(
        ir.contains("call double @js_object_get_field_value_site("),
        "dispatching wrappers must still be emitted in a default build \
         (property get):\n{ir}"
    );
    assert!(
        ir.contains("call double @js_typed_feedback_native_call_method")
            || ir.contains("call double @js_method_call_slow("),
        "dispatching wrappers must still be emitted in a default build \
         (method call):\n{ir}"
    );
    assert!(
        ir.contains("call i32 @perry_probe_set(") && ir.contains("call double @js_put_value_set("),
        "the by-name store itself must still be lowered in a default build:\n{ir}"
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

    // #12483: a class-field store is the static-key store site and a read the
    // layout probe: a class-typed PARAMETER proves nothing about the live
    // receiver, so the site guards nothing itself. One store probe (hit) with the
    // receiver-aware `[[Set]]` behind it; one value probe with the complete
    // `[[Get]]` behind it. No class-field guard, no inline slot access.
    assert!(ir.contains("@perry_typed_shape_raw_f64_mask_"));
    assert!(ir.contains("call i32 @perry_probe_set("));
    assert!(ir.contains("call double @js_put_value_set("));
    assert!(ir.contains("put.pic.set") && ir.contains("put.pic.merge"));
    for gone in [
        "class_field_set_guard",
        "js_typed_feedback_class_field_set_guard",
        "class_field_set.fast",
        "class_field_set.fallback",
        "class_field_get.fast",
        "class_field_inline.deref",
        "call double @js_class_field_get_ic(",
        "call void @js_class_field_set_fallback",
        "call i32 @js_typed_feedback_class_field_get_guard(",
    ] {
        assert!(
            !ir.contains(gone),
            "the retired class-field form `{gone}`:\n{ir}"
        );
    }
    assert!(ir.contains("call double @perry_probe_get_value("));
    assert!(ir.contains("call double @js_object_get_field_value("));
    assert!(!ir.contains("call void @js_gc_note_slot_layout"));
    assert!(!ir.contains("call void @js_object_set_field_by_name"));
    // The recorders are emitted only in a typed-feedback build.
    assert!(!ir.contains("call void @js_typed_feedback_record_fallback_call"));
    assert!(!ir.contains("call double @js_object_get_field_by_name_f64("));
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
    // #5334 lever B / #12483: a class-field store is the static-key store
    // site, so the full outline collapses it as it collapses any store: to the
    // site's GC-leaf hit plus a cold collecting miss. No class guard either way.
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

    // The store site is ALREADY the collapsed form in both modes: one GC-leaf
    // `perry_probe_set` (the hit) and a cold collecting `js_put_value_set` (the
    // miss), with no per-site cache state or class guard to outline. Forcing the
    // full outline therefore changes nothing for it.
    for mode in ["1", "0"] {
        let _g = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", Some(mode));
        let ir = ir_for(build());
        assert_eq!(
            ir.matches("call i32 @perry_probe_set(").count(),
            1,
            "full-outline={mode}: one store probe:\n{ir}"
        );
        assert!(
            ir.contains("call double @js_put_value_set("),
            "full-outline={mode}: the cold miss is the complete [[Set]]:\n{ir}"
        );
        assert!(!ir.contains("@js_put_value_set_packed_fast("), "{mode}");
        assert!(!ir.contains("put.pic.hit.store"), "{mode}");
        assert!(!ir.contains("class_field_set."), "{mode}");
        assert!(!ir.contains("class_field_inline."), "{mode}");
    }
}

#[test]
fn full_outline_ic_collapses_class_field_get_to_single_call() {
    // #5391 path 2 / #12483: a class-field read is a shape-site read, so the
    // full outline collapses it exactly as it collapses any property read: to
    // the site's GC-leaf hit plus a cold collecting miss.
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

    // Both modes keep one leaf and a complete collecting miss. #12986 selects
    // a validated memo in the outlined mode; the non-outlined own field keeps
    // the stateless entrance. No memo storage belongs to the latter site.
    for mode in ["1", "0"] {
        let _g = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", Some(mode));
        let ir = ir_for(build());
        // One probe per lowering of the body (the guarded clone and the
        // generic body each hold exactly one site).
        let entrance = if mode == "1" {
            "call double @perry_probe_get_value_memo("
        } else {
            "call double @perry_probe_get_value("
        };
        let probes = ir.matches(entrance).count();
        assert!(
            (1..=2).contains(&probes),
            "full-outline={mode}: one value probe per body, saw {probes}:\n{ir}"
        );
        if mode == "1" {
            assert_eq!(
                read_memo_contract::paired_memo_reads(&ir),
                Some(probes),
                "{ir}"
            );
        } else {
            assert!(
                ir.contains("call double @js_object_get_field_value("),
                "{ir}"
            );
            assert_eq!(read_memo_contract::paired_memo_reads(&ir), None, "{ir}");
            assert!(
                !ir.contains(" = private global [5 x i64] zeroinitializer"),
                "{ir}"
            );
        }
        assert!(!ir.contains("_packed_get"), "{mode}");
        assert!(
            !ir.contains("call double @js_object_get_field_ic_fast("),
            "{mode}"
        );
        assert!(!ir.contains("class_field_get.fast"), "{mode}");
        assert!(!ir.contains("class_field_inline.deref"), "{mode}");
        assert!(
            !ir.contains("call i32 @js_typed_feedback_class_field_get_guard("),
            "{mode}"
        );
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
            vec![Stmt::Return(Some(Expr::Array(
                vec![Expr::LocalGet(1), Expr::Number(2.0), Expr::LocalGet(1)],
                None,
            )))],
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
    //
    // The observable is the array-literal construction, the one site the full
    // outline still changes (a class-field store/read is already the one probe
    // site in both modes): outlined, it is the `js_array_from_values` builder
    // call; inline, it is the nursery-bump diamond.
    let build = |methods: u32| {
        let mut big = class(150, "Big", vec![field("x", Type::Number)]);
        for i in 0..methods {
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
        // A param-based array (not all-const) so it reaches the array-literal
        // lowering rather than const-folding to a rodata global.
        module_with_classes(
            "auto_gate.ts",
            vec![big],
            vec![param(1, "x", Type::Number)],
            Type::Any,
            vec![Stmt::Return(Some(Expr::Array(
                vec![Expr::LocalGet(1), Expr::Number(2.0), Expr::LocalGet(1)],
                None,
            )))],
        )
    };

    let _lock = env_lock();
    // Auto path (override unset), threshold 5.
    let _ic = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", None);
    let _min = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC_MIN_FUNCS", Some("5"));
    // 1 probe fn + 6 methods = 7 callables clears the threshold even though
    // `hir.functions.len()` is just 1. The pre-fix function-only count (1)
    // would have stayed under it.
    let tripped = ir_for(build(6));
    assert!(
        tripped.contains("call i64 @js_array_from_values"),
        "7 callables (1 function + 6 methods) must trip the gate:\n{tripped}"
    );
    // Control: 1 function + 1 method = 2 callables stays inline.
    let inline = ir_for(build(1));
    assert!(
        !inline.contains("call i64 @js_array_from_values"),
        "2 callables must stay under the gate:\n{inline}"
    );
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

    // #12483: a class-field store is the static-key store site, so the write
    // barrier for a boxed (String) slot lives in the runtime's store probe, not
    // in the compiled site: neither value emits an inline slot barrier or a
    // class-field guard, and BOTH hand their value to the probe. (A definite
    // heap pointer is therefore barriered by the runtime exactly like any
    // other; the compiled code cannot elide, or omit, the barrier.)
    for (val, what) in [
        (Expr::Number(7.0), "numeric"),
        (Expr::String("hi".to_string()), "heap pointer"),
    ] {
        let ir = ir_for(build(val));
        assert!(ir.contains("call i32 @perry_probe_set("), "{what}:\n{ir}");
        assert!(
            ir.contains("call double @js_put_value_set("),
            "{what}:\n{ir}"
        );
        assert!(!ir.contains("class_field_set.fast"), "{what}:\n{ir}");
        assert!(
            !ir.contains("call void @js_write_barrier_slot"),
            "{what}: the barrier is the runtime's:\n{ir}"
        );
    }
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
    // Class field initialization follows DefineField semantics, and a class
    // field store is the static-key store site (#12483): the constructor
    // defines the field with `js_class_field_add` (never an inherited setter's
    // [[Set]]), and no probe-site guard around a field store exists. The
    // helper is the CONSTRUCTOR's; the method call site below never calls it.
    assert!(!ir.contains("class_field_set.fast"));
    assert!(!ir.contains("class_field_set.fallback"));
    assert!(!ir.contains("js_typed_feedback_class_field_set_guard("));
    let ctor_start = ir
        .find("define double @typed_feedback_class_method_ts__Point_constructor")
        .expect("the class constructor is emitted");
    let ctor = &ir[ctor_start..ctor_start + ir[ctor_start..].find("\n}\n").unwrap()];
    assert!(ctor.contains("call double @js_class_field_add("));
    let probe_start = ir
        .find("define double @perry_fn_typed_feedback_class_method_ts__probe")
        .expect("the probe is emitted");
    let probe = &ir[probe_start..probe_start + ir[probe_start..].find("\n}\n").unwrap()];
    assert!(!probe.contains("call double @js_class_field_add("));
    // The generic arm of the direct method site is the id-keyed dynamic call.
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
    counter.methods.push(Function {
        id: 9,
        name: "identity".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: 4,
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
        }],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::LocalGet(4)))],
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
        probe_ir.contains(
            "call double @perry_method_synthetic_arguments_method_guard_ts__Counter__count$arguments_length",
        ) && probe_ir.contains("double 1.0"),
        "the guarded direct arm should pass the actual argument count to the additive clone:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_array_alloc")
            && !probe_ir.contains("call i64 @js_array_push_f64")
            && !probe_ir.contains("call i64 @js_array_mark_arguments_object"),
        "a length-only direct call must not allocate or fill an argument bundle:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call double @js_native_call_method_by_id")
            && !probe_ir.contains("call double @js_object_get_own_field_or_undef"),
        "a guard miss should preserve dynamic override dispatch without an eager own-property scan:\n{probe_ir}"
    );
    assert!(
        ir.contains(
            "define double @perry_method_synthetic_arguments_method_guard_ts__Counter__count$arguments_length",
        ) && !ir.contains(
            "perry_method_synthetic_arguments_method_guard_ts__Counter__identity$arguments_length",
        ),
        "only the producer-proved length-only method may publish the scalar ABI:\n{ir}"
    );
}

#[test]
fn typed_feedback_guards_direct_closure_call_specialization() {
    // This test asserts the non-recording lane. Join the env-mutating tests'
    // lock and pin that lane, including when the parent enables profiling.
    let _lock = env_lock();
    let _feedback = EnvVarGuard::set("PERRY_TYPED_FEEDBACK", None);
    let _trace = EnvVarGuard::set("PERRY_TYPED_FEEDBACK_TRACE", None);
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
                init: Some(Expr::Array(Vec::new(), None)),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: 1,
                value: Box::new(Expr::Number(7.0)),
                field_writeback: None,
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
    // this test always observes the inline numeric-array header rather than
    // the outlined builder. Canonical doubles set their layout at birth:
    // normalization would be unreachable and must not reserve a root slot.
    let _lock = env_lock();
    let _g = EnvVarGuard::set("PERRY_FULL_OUTLINE_IC", Some("0"));
    let numeric_ir = ir_for(module(
        "typed_feedback_numeric_array_literal.ts",
        Vec::new(),
        Type::Any,
        vec![Stmt::Return(Some(Expr::Array(
            vec![
                Expr::Number(1.0),
                Expr::Integer(2),
                Expr::Binary {
                    op: perry_hir::BinaryOp::Mul,
                    left: Box::new(Expr::Number(3.0)),
                    right: Box::new(Expr::Number(4.0)),
                },
            ],
            None,
        )))],
    ));

    // GcHeader: size=56 (8-byte GcHeader, 24-byte ArrayHeader with its
    // `named` word, three slots), pointer-free/raw-f64 reserved bits=0x4080,
    // flags=ARENA, type=ARRAY. Live birth flags are ORed into this word.
    let numeric_header = (56u64 << 32) | (0x4080u64 << 16) | 0x0201;
    assert!(numeric_ir.contains(&numeric_header.to_string()));
    assert!(!numeric_ir.contains("call i32 @js_array_mark_numeric_f64_layout"));

    let mixed_ir = ir_for(module(
        "typed_feedback_mixed_array_literal.ts",
        Vec::new(),
        Type::Any,
        vec![Stmt::Return(Some(Expr::Array(
            vec![Expr::Number(1.0), Expr::String("x".to_string())],
            None,
        )))],
    ));

    assert!(!mixed_ir.contains("call i32 @js_array_mark_numeric_f64_layout"));
    let mixed_raw_f64_header = (48u64 << 32) | (0x4080u64 << 16) | 0x0201;
    assert!(!mixed_ir.contains(&mixed_raw_f64_header.to_string()));
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
                init: Some(Expr::Array(vec![Expr::Number(1.0)], None)),
            },
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(2)),
                index: Box::new(Expr::Number(0.0)),
                value: Box::new(Expr::String("not-number".to_string())),
            }),
            Stmt::Expr(Expr::ArrayPush {
                array_id: 2,
                value: Box::new(Expr::String("still-not-number".to_string())),
                field_writeback: None,
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

    // The default lowering of a number[] read is the inline guard (receiver
    // header, bounds, backing: `arr.guard.*`) with the slot loaded in place and
    // checked to be a plain number; any miss takes the shared cold read.
    assert!(ir.contains("\narr.guard.deref."));
    assert!(ir.contains("\narr.guard.range."));
    assert!(ir.contains("\narr.guard.backing."));
    assert!(ir.contains("call double @perry_array_read_cold_shared("));
    // The slot is read straight from the element address; the registered
    // guard/fallback pair is the profile-replay form, not this one.
    assert!(!ir.contains("call double @js_array_numeric_get_f64_unboxed"));
    assert!(!ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard"));
    assert!(ir.contains("load i64, ptr"));
}

#[test]
fn profile_replay_selects_numeric_read_with_guard_fallback_and_deterministic_ir() {
    use perry_codegen::typed_feedback_profile::{ModuleIdentity, Profile, Session};
    let _lock = env_lock();
    let _feedback = EnvVarGuard::set("PERRY_TYPED_FEEDBACK", None);
    let _trace = EnvVarGuard::set("PERRY_TYPED_FEEDBACK_TRACE", None);
    let dir = std::env::temp_dir().join(format!("perry-replay-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = module(
        "replay.ts",
        vec![param(1, "xs", Type::Array(Box::new(Type::Any)))],
        Type::Any,
        vec![Stmt::Return(Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Number(0.0)),
        }))],
    );
    let identity = ModuleIdentity {
        module: source.name.clone(),
        source_hash: "source".into(),
        hir_hash: "hir".into(),
        lowering_hash: "opts".into(),
        target: "host".into(),
    };
    let mut opts = empty_opts();
    opts.verify_native_regions = true;
    let catalog = Session::new("compiler".into(), None);
    let baseline = catalog
        .compile_module(&source, opts.clone(), identity.clone())
        .unwrap();
    catalog.finish(Some(&dir.join("sites.json"))).unwrap();
    let mut profile: Profile = Session::read_profile(&dir.join("sites.json")).unwrap();
    let sites = &mut profile.modules[0].sites;
    sites.retain(|site| site.kind == "array_element" && site.operation == "array[index]");
    assert!(
        !sites.is_empty(),
        "fixture must reach a supported array read"
    );
    for site in sites {
        site.observation_kind = "numeric_array_element".into();
    }
    let _reps = EnvVarGuard::set("PERRY_NATIVE_REPS", Some("1"));
    let _reps_dir = EnvVarGuard::set("PERRY_NATIVE_REPS_DIR", Some(dir.to_str().unwrap()));
    let replay = Session::new("compiler".into(), Some(profile.clone()));
    let selected = replay
        .compile_module(&source, opts.clone(), identity.clone())
        .unwrap();
    let decisions = replay.finish(None).unwrap();
    assert!(decisions.iter().any(|d| d.accepted), "{decisions:?}");
    let ir = String::from_utf8(selected.clone()).unwrap();
    // The default read checks the loaded word for the hole (an absent element
    // goes to the cold read); the replayed numeric-element observation selects
    // the stronger test instead: the word must be a plain number, and anything
    // else (a hole included) takes the same cold read. Both keep the cold read
    // as the fallback arm.
    let fast = block_body(&ir, "arr.fast.").expect("the selected read has its fast arm");
    let base_ir = String::from_utf8(baseline.clone()).unwrap();
    let base_fast = block_body(&base_ir, "arr.fast.").expect("the default read has its fast arm");
    assert!(
        fast.contains("and i64 %")
            && fast.contains("icmp ule i64 %")
            && fast.contains(", 9221401712017801215"),
        "the selected numeric read must test for a plain number:\n{fast}"
    );
    assert!(
        !base_fast.contains(", 9221401712017801215"),
        "the default read does not test for a plain number:\n{base_fast}"
    );
    assert!(ir.contains("call double @perry_array_read_cold_shared("));
    assert!(!ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard"));
    assert!(ir.contains("br i1"));
    assert_ne!(baseline, selected);
    let replay2 = Session::new("compiler".into(), Some(profile.clone()));
    assert_eq!(
        selected,
        replay2
            .compile_module(&source, opts.clone(), identity.clone())
            .unwrap()
    );
    assert_eq!(decisions, replay2.finish(None).unwrap());
    // Every well-formed mismatch must leave lowering byte-for-byte identical.
    let cases: &[(&str, fn(&mut Profile))] = &[
        ("source_hash_mismatch", |p| {
            p.modules[0].identity.source_hash.push('x')
        }),
        ("hir_hash_mismatch", |p| {
            p.modules[0].identity.hir_hash.push('x')
        }),
        ("lowering_inputs_mismatch", |p| {
            p.modules[0].identity.lowering_hash.push('x')
        }),
        ("target_mismatch", |p| {
            p.modules[0].identity.target.push('x')
        }),
        ("compiler_mismatch", |p| p.compiler.push('x')),
        ("schema_mismatch", |p| p.schema_version += 1),
        ("unknown_module", |p| p.modules[0].identity.module.push('x')),
        ("unknown_site", |p| {
            for s in &mut p.modules[0].sites {
                s.site_id += 1000;
            }
        }),
        ("site_identity_mismatch", |p| {
            for s in &mut p.modules[0].sites {
                s.function.push('x');
            }
        }),
        ("unsupported_observation_kind", |p| {
            for s in &mut p.modules[0].sites {
                s.observation_kind = "method_address".into();
            }
        }),
    ];
    for (reason, mutate) in cases {
        let mut stale_profile = profile.clone();
        mutate(&mut stale_profile);
        let stale = Session::new("compiler".into(), Some(stale_profile));
        assert_eq!(
            baseline,
            stale
                .compile_module(&source, opts.clone(), identity.clone())
                .unwrap(),
            "{reason}"
        );
        let rejected = stale.finish(None).unwrap();
        assert!(!rejected.is_empty(), "{reason}");
        assert!(
            rejected.iter().all(|d| !d.accepted && d.reason == *reason),
            "{rejected:?}"
        );
    }
    let artifacts: Vec<serde_json::Value> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            (path.extension().and_then(|s| s.to_str()) == Some("json")
                && path.file_name().unwrap() != "sites.json")
                .then(|| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())
        })
        .collect();
    assert!(artifacts
        .iter()
        .any(|a| a["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["consumed_facts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["kind"] == "typed_feedback_replay"))));
    std::fs::remove_dir_all(&dir).unwrap();
}
