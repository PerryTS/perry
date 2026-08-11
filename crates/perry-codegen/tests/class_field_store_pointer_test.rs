//! #7511 — the class-field store's GC bookkeeping sits behind ONE inline,
//! live test of the stored value's bits.
//!
//! The subject is `expr::write_barrier::emit_jsvalue_slot_store_pointer_tested`.
//! What these tests have to pin is not "the calls got faster" but the two
//! structural properties that make the guard sound:
//!
//! 1. the SLOT STORE is unconditional and stays outside the guarded block, and
//! 2. all three bookkeeping calls — write barrier, layout note, string addref —
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

fn compile_ir(module: &Module) -> String {
    String::from_utf8(compile_module(module, empty_opts()).unwrap()).expect("IR should be UTF-8")
}

/// The body of the guarded block: every line after the
/// `class_field_set.gc_bookkeeping.<n>:` LABEL up to (and excluding) its
/// terminator. `None` when no guard was emitted.
///
/// Matched on the label DEFINITION, never on the first textual occurrence of
/// the name — that is the `br i1 …, label %class_field_set.gc_bookkeeping.N,
/// label %class_field_set.gc_bookkeeping.done.M` a line above, and starting
/// there yields an empty slice that passes nothing and fails everything.
/// The guarded REGION: every block reachable from the `gc_bookkeeping` entry
/// without passing through `gc_bookkeeping.done`, i.e. exactly the blocks the
/// pointer-bearing guard dominates.
///
/// This used to be a text slice -- from the `gc_bookkeeping.N:` label to the
/// first `br` -- which is the same thing only while the region is ONE block.
/// #5094 made it three (`gc_bookkeeping` -> `layout_note` / `layout_note.done`
/// -> `gc_bookkeeping.done`), and the slice then returned only the prefix, so
/// `@js_write_barrier_slot` "left the guarded block" without moving at all.
///
/// A wider text slice would not do either: LLVM emits `gc_bookkeeping.done.5`
/// **between** `gc_bookkeeping.4` and `layout_note.6`, so the region is not
/// textually contiguous and "everything up to the done label" is still just
/// the first block.
///
/// Following the CFG also makes the assertion strictly stronger than the one
/// it replaces: a call hoisted onto the guard's NOT-taken edge, or past the
/// join into `merge`, is absent from the region and fails -- whereas the old
/// slice only ever proved a call was not in the first block.
fn gc_bookkeeping_block(ir: &str) -> Option<String> {
    // label -> (body lines, successor labels)
    let mut blocks: Vec<(String, Vec<&str>, Vec<String>)> = Vec::new();
    for line in ir.lines() {
        let trimmed = line.trim_end();
        if !line.starts_with(char::is_whitespace) && trimmed.ends_with(':') {
            blocks.push((
                trimmed.trim_end_matches(':').to_string(),
                Vec::new(),
                Vec::new(),
            ));
            continue;
        }
        let Some((_, body, succs)) = blocks.last_mut() else {
            continue;
        };
        body.push(line);
        if trimmed.trim_start().starts_with("br ") {
            for token in trimmed.split("label %").skip(1) {
                succs.push(
                    token
                        .split(|c: char| c == ',' || c.is_whitespace())
                        .next()
                        .unwrap_or("")
                        .to_string(),
                );
            }
        }
    }

    let entry = blocks.iter().position(|(label, _, _)| {
        label.starts_with("class_field_set.gc_bookkeeping.")
            && !label.starts_with("class_field_set.gc_bookkeeping.done")
    })?;

    let mut region = String::new();
    let mut stack = vec![entry];
    let mut seen = vec![false; blocks.len()];
    while let Some(idx) = stack.pop() {
        if seen[idx] {
            continue;
        }
        seen[idx] = true;
        let (_, body, succs) = &blocks[idx];
        for line in body {
            region.push_str(line);
            region.push('\n');
        }
        for succ in succs {
            // The join is the region's exit, not part of it.
            if succ.starts_with("class_field_set.gc_bookkeeping.done") {
                continue;
            }
            if let Some(next) = blocks.iter().position(|(label, _, _)| label == succ) {
                stack.push(next);
            }
        }
    }
    Some(region)
}

/// An `any`-typed field is the case the whole ticket is about: it takes the
/// boxed store path, so the three bookkeeping calls exist, and the value is a
/// constructor PARAMETER, so no by-construction proof can retire them.
#[test]
fn opaque_param_store_guards_all_three_bookkeeping_calls() {
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

    // The guard exists, and it is the top-16-bit test — not something that
    // happens to look like one. `32765`/`32767`/`32762` are POINTER/STRING/
    // BIGINT `>> 48`; `4096` is the bare-address floor.
    assert!(
        ir.contains("lshr i64") && ir.contains("class_field_set.gc_bookkeeping"),
        "expected an inline pointer-bearing guard around the field-store bookkeeping:\n{ir}"
    );
    for comparand in ["32765", "32767", "32762", "4096"] {
        assert!(
            ir.contains(comparand),
            "the inline guard is missing the {comparand} comparand:\n{ir}"
        );
    }

    let guarded = gc_bookkeeping_block(&ir).expect("a gc_bookkeeping block");
    for call in [
        "@js_write_barrier_slot",
        "@js_gc_note_slot_layout",
        "@js_string_addref_if_heap_string",
    ] {
        assert!(
            guarded.contains(call),
            "{call} must live inside the guarded block, not beside it:\n{guarded}"
        );
    }
    // …and the slot store must NOT, or a value would be dropped whenever the
    // guard says "no pointer".
    assert!(
        !guarded.contains("store double"),
        "the slot store must stay unconditional, outside the guard:\n{guarded}"
    );
}

/// **The boundary.** The guard replaces a *runtime* early-out, so it must never
/// remove a call outright: a module that can store a pointer still has all
/// three calls present in the emitted IR, reachable on the guard's taken edge.
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
    for call in [
        "call void @js_write_barrier_slot",
        "call void @js_gc_note_slot_layout",
        "call void @js_string_addref_if_heap_string",
    ] {
        assert!(ir.contains(call), "{call} must still be emitted:\n{ir}");
    }
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
    let guarded = gc_bookkeeping_block(&ir)
        .unwrap_or_else(|| panic!("expected a guarded bookkeeping block in:\n{ir}"));
    for call in [
        "@js_write_barrier_slot",
        "@js_gc_note_slot_layout",
        "@js_string_addref_if_heap_string",
    ] {
        assert!(
            guarded.contains(call),
            "{call} must live inside the guarded block:\n{guarded}"
        );
    }

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

/// A `number`-declared field takes the raw-f64 store path, which is proven
/// pointer-free by the typed shape descriptor and never had bookkeeping to
/// guard. Asserting the guard is ABSENT there keeps the test above from being
/// satisfied by "codegen emits this block everywhere".
#[test]
fn raw_f64_field_store_emits_no_guard() {
    assert_default_barrier_env_not_disabled();
    let module = module_with_new(
        class(
            3,
            "Numeric",
            vec![field("v", Type::Number)],
            Some(param_prologue_ctor("v", 7, Type::Number)),
        ),
        vec![Expr::Number(1.0)],
    );
    let ir = compile_ir(&module);
    assert!(
        !ir.contains("class_field_set.gc_bookkeeping"),
        "a raw-f64 class-field store has no bookkeeping to guard:\n{ir}"
    );
}
