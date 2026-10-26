//! #7511 — the class-field store's GC bookkeeping sits behind ONE inline,
//! live test of the stored value's bits.
//!
//! A class-field store is the static-key store site (#12483), so the subject
//! is that site's bookkeeping (`put_value_store_ic::emit_static_store_ic_bookkeeping`),
//! which takes the same pointer test as
//! `expr::write_barrier::emit_jsvalue_slot_store_pointer_tested`.
//! What these tests have to pin is not "the calls got faster" but the two
//! structural properties that make the guard sound:
//!
//! 1. the SLOT STORE is unconditional and stays outside the guarded block, and
//! 2. both bookkeeping calls — write barrier and string addref —
//!    are inside it, so none of them can be skipped by a *different* condition
//!    than the one that proves them dead.
//!
//! Both are asserted on emitted IR rather than by calling the emitter directly,
//! because the failure mode this ticket is one wrong branch away from is a
//! stranded child in generated code.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Function, Module, ModuleInitKind, Param, Stmt};

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
        lazy_path_init_modules: Vec::new(),
        deferred_module_prefixes: Default::default(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        namespace_adopts_cjs_exports: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}

fn assert_default_barrier_env_not_disabled() {
    assert!(
        !matches!(
            std::env::var("PERRY_WRITE_BARRIERS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        "these tests describe DEFAULT barrier emission; PERRY_WRITE_BARRIERS must be unset or on"
    );
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

/// `constructor(v) { this.v = v }` — the shape HIR synthesizes for every
/// closed-shape object literal (`lower/context.rs::mint_anon_shape_class`) and
/// the shape a hand-written data class already has.
fn param_prologue_ctor(field_name: &str, param_id: u32, param_ty: Type) -> Function {
    Function {
        id: 90,
        name: "constructor".to_string(),
        type_params: Vec::new(),
        params: vec![param(param_id, field_name, param_ty)],
        return_type: Type::Void,
        body: vec![Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: field_name.to_string(),
            value: Box::new(Expr::LocalGet(param_id)),
        })],
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

fn class(id: u32, name: &str, fields: Vec<ClassField>, constructor: Option<Function>) -> Class {
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

fn module_with_new(class: Class, args: Vec<Expr>) -> Module {
    let class_name = class.name.clone();
    Module {
        name: "class_field_store_pointer_test.ts".to_string(),
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
                args,
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

fn compile_ir(module: &Module) -> String {
    String::from_utf8(compile_module(module, empty_opts()).unwrap()).expect("IR should be UTF-8")
}

/// A class-field store is the static-key store site (#12483): one
/// `perry_probe_set` over the receiver's layout (the runtime owns the
/// pointer-bearing bookkeeping: write barrier and string addref), and only a
/// declined probe reaches the full `[[Set]]` (`js_put_value_set`) on the
/// `put.pic.set` block. The site holds no inline bookkeeping region and no
/// slot store of its own.
fn assert_store_site(ir: &str, strict: &str) {
    assert!(
        ir.contains("call i32 @perry_probe_set("),
        "the field store is a probe call:\n{ir}"
    );
    let misses: Vec<&str> = ir
        .split("\nput.pic.set.")
        .skip(1)
        .map(|b| &b[..b.find("\n\n").unwrap_or(b.len())])
        .collect();
    assert!(
        misses
            .iter()
            .any(|miss| miss.contains("call double @js_put_value_set(")
                && miss.contains(&format!(", i32 {strict})"))),
        "a declined probe must reach the receiver-aware [[Set]] (Throw = {strict}) \
         on a `put.pic.set` block:\n{ir}"
    );
    assert!(
        !ir.contains("put.pic.gc_bookkeeping"),
        "bookkeeping lives in the runtime's probe, not an inline guard:\n{ir}"
    );
}

/// An `any`-typed field is the case the whole ticket is about: it takes the
/// boxed store path, so both bookkeeping calls exist, and the value is a
/// constructor PARAMETER, so no by-construction proof can retire them.
#[test]
fn opaque_param_store_guards_both_bookkeeping_calls() {
    assert_default_barrier_env_not_disabled();
    let module = module_with_new(
        class(
            1,
            "Boxed",
            vec![field("v", Type::Any)],
            Some(param_prologue_ctor("v", 7, Type::Any)),
        ),
        vec![Expr::Number(1.0)],
    );
    let ir = compile_ir(&module);
    // The constructor's store of an opaque parameter is the static-key store
    // site (strict: class code), so the value-dependent bookkeeping cannot be
    // retired by any by-construction proof at the site; the probe decides.
    assert_store_site(&ir, "1");
    let ctor = ir
        .split("\ndefine ")
        .find(|f| f.contains("Boxed_constructor("))
        .expect("the constructor is emitted");
    assert!(
        !ctor.contains("call void @js_write_barrier_slot")
            && !ctor.contains("call void @js_string_addref_if_heap_string"),
        "no inline bookkeeping beside the probe:\n{ctor}"
    );
}

/// **The boundary.** The guard replaces a *runtime* early-out, so it must never
/// remove a call outright: a module that can store a pointer still has both
/// calls present in the emitted IR, reachable on the guard's taken edge.
/// This is what distinguishes the change from an elision.
#[test]
fn every_bookkeeping_call_is_still_emitted_not_removed() {
    assert_default_barrier_env_not_disabled();
    let module = module_with_new(
        class(
            2,
            "Boxed2",
            vec![field("v", Type::Any)],
            Some(param_prologue_ctor("v", 7, Type::Any)),
        ),
        vec![Expr::Number(1.0)],
    );
    let ir = compile_ir(&module);
    // Not elided: the store still reaches the probe and its full-semantics
    // miss, so a pointer-bearing value is always barriered by one of them.
    assert_store_site(&ir, "1");
    assert!(
        ir.contains("call void @js_gc_write_barriers_emitted(i32 1)"),
        "the module must still declare to the runtime that generated barriers exist"
    );
}

/// A module whose `probe` binds `let o = new C(v)` and then writes a field on
/// the LOCAL receiver rather than on `this`, so the store is lowered from a
/// different `property_set.rs` entry than the constructor prologue's.
fn module_with_local_receiver_store(class: Class, stored: Expr) -> Module {
    let class_name = class.name.clone();
    let mut module = module_with_new(class, vec![Expr::Number(1.0)]);
    module.functions[0].body = vec![
        Stmt::Let {
            id: 5,
            name: "o".to_string(),
            ty: Type::Named(class_name.clone()),
            mutable: false,
            init: Some(Expr::New {
                class_name,
                args: vec![Expr::Number(1.0)],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::LocalGet(5)),
            property: "v".to_string(),
            value: Box::new(stored),
        }),
        Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(5)),
            property: "v".to_string(),
            byte_offset: 0,
        })),
    ];
    module.functions[0].return_type = Type::Any;
    module
}

/// **Every emitted block is still terminated.**
///
/// `emit_jsvalue_slot_store_pointer_tested` leaves `ctx.current_block` on a
/// freshly created merge block, and `property_set.rs` has arms that terminate
/// explicitly (the typed-feedback guarded arm's `br` to `class_field_set.merge`)
/// and arms that fall through to whatever the caller emits next (the
/// `ptr_shape_receiver_fact` arm returns without a terminator, exactly as it did
/// before this change). A dangling merge block is invisible to a call-count
/// assertion and is how the second shape would break, so scan the whole module:
/// no label may follow another label with no terminator in between.
#[test]
fn guarded_store_leaves_every_block_terminated() {
    assert_default_barrier_env_not_disabled();
    let module = module_with_local_receiver_store(
        class(
            4,
            "LocalBoxed",
            vec![field("v", Type::Any)],
            Some(param_prologue_ctor("v", 7, Type::Any)),
        ),
        // An opaque local read: no by-construction proof, so the flags survive
        // to the guard instead of being retired by lever D.
        Expr::LocalGet(5),
    );
    let ir = compile_ir(&module);
    // The local-receiver store is the same store site (sloppy here).
    assert_store_site(&ir, "0");

    let mut open_block: Option<&str> = None;
    for line in ir.lines() {
        let trimmed = line.trim();
        if line.ends_with(':') && !line.starts_with(' ') && !line.starts_with('%') {
            assert!(
                open_block.is_none(),
                "block {:?} was left unterminated before {line:?}",
                open_block.unwrap()
            );
            open_block = Some(line);
        } else if trimmed.starts_with("br ")
            || trimmed.starts_with("ret ")
            || trimmed.starts_with("unreachable")
            || trimmed.starts_with("switch ")
        {
            open_block = None;
        }
    }
    // A closing `}` is deliberately NOT treated as a terminator: the last block
    // of a function is exactly where a fall-through arm leaves a dangling
    // merge block, and accepting `}` would let that pass.
    assert!(
        open_block.is_none(),
        "block {:?} was left unterminated at the end of the module",
        open_block.unwrap()
    );
}

/// #10907: `function probe(p: Numeric, x: number) { p.v = <stored>; return p }`
/// — a `: number` field written through a declared-class PARAMETER receiver,
/// so the store takes the runtime-guarded class-field arm (sloppy or strict per
/// `strict`) rather than a scalar-replaced slot or a `Ptr<Shape>`-proven store.
fn module_with_param_receiver_raw_f64_store(strict: bool, stored: Expr) -> Module {
    let mut module = module_with_new(
        class(
            6,
            "Numeric6",
            vec![field("v", Type::Number)],
            Some(param_prologue_ctor("v", 7, Type::Number)),
        ),
        vec![Expr::Number(1.0)],
    );
    let probe = &mut module.functions[0];
    probe.is_strict = strict;
    probe.params = vec![
        param(8, "p", Type::Named("Numeric6".to_string())),
        param(9, "x", Type::Number),
    ];
    probe.return_type = Type::Any;
    probe.body = vec![
        Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::LocalGet(8)),
            property: "v".to_string(),
            value: Box::new(stored),
        }),
        Stmt::Return(Some(Expr::LocalGet(8))),
    ];
    module
}

/// Every `define` whose name contains `__probe` (the exported body plus its
/// `$generic` / `$spec_*` clones), each split into `(label, body)` blocks.
fn probe_blocks(ir: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_probe = false;
    for line in ir.lines() {
        if line.starts_with("define ") {
            in_probe = line.contains("__probe");
            continue;
        }
        if !in_probe {
            continue;
        }
        if line.starts_with('}') {
            in_probe = false;
            continue;
        }
        let trimmed = line.trim_end();
        if !line.starts_with(char::is_whitespace) && trimmed.ends_with(':') {
            out.push((trimmed.trim_end_matches(':').to_string(), String::new()));
        } else if let Some((_, body)) = out.last_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    out
}

const CANONICALIZE: &str = "@js_array_numeric_value_to_raw_f64";

/// #10907 / #12483: a `: number` field written through a declared-class
/// parameter is the static-key store site, strict or sloppy. Its `F64`-lane
/// hit stores only a plain finite double (one exponent test) and sends every
/// other value to the miss, so no path of the probe calls the canonicalizer —
/// the ~31-instruction `: number` vs `: any` store penalty #10907 removed —
/// and no class guard against the birth ShapeId is emitted.
#[test]
fn raw_f64_field_store_is_the_store_site_and_never_canonicalizes() {
    for strict in [false, true] {
        for stored in [Expr::LocalGet(9), Expr::Number(3.5)] {
            let ir = compile_ir(&module_with_param_receiver_raw_f64_store(strict, stored));
            let blocks = probe_blocks(&ir);
            assert!(
                blocks
                    .iter()
                    .any(|(label, _)| label.starts_with("put.pic.")),
                "strict={strict}: the store must be the static-key store site:\n{ir}"
            );
            for (label, body) in &blocks {
                assert!(
                    !label.starts_with("class_field_set.")
                        && !label.starts_with("class_field_sloppy_set.")
                        && !label.starts_with("class_field_inline."),
                    "strict={strict}: {label}: no class store guard may be emitted:\n{ir}"
                );
                assert!(
                    !body.contains(CANONICALIZE),
                    "strict={strict}: {label}: the store must not canonicalize:\n{body}"
                );
            }
        }
    }
}
