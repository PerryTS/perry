//! #7871: a self-recursive function's `new` sites take the INLINE bump
//! allocator.
//!
//! The subject is [`super::new_alloc::new_site_is_in_loop`]'s second arm and the
//! `collectors::collect_alloc_hot_functions` set behind it.
//!
//! This is a liveness gate, and it exists because the failure mode is silence.
//! `js_object_alloc_class_inline_keys` performs the identical bump alloc and
//! returns the identical user pointer, so a gate that stops firing changes no
//! output, breaks no other test, and simply makes every allocation in a
//! recursive-descent evaluator cost a cross-crate call again — `interp.ts`'s
//! −22.9% quietly evaporating with nothing to show for it. Only the emitted
//! `alloc.fast` / `js_inline_arena_slow_alloc` shape separates the two.
//!
//! The negative half is the anti-bloat property: a `new` in a function that is
//! neither in a loop, nor called from one, nor recursive keeps the outlined
//! call. Without it this file would pass just as happily if the gate had been
//! widened to "always", which is the ~268-bytes-per-site default the
//! `[#bloat]` comment in `new_alloc.rs` exists to refuse.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, Class, ClassField, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt,
};

/// Emitted only by the inline bump allocator. Both are CALL/LABEL forms, not
/// bare symbol names: `runtime_decls` emits a `declare` for
/// `js_inline_arena_slow_alloc` into every module whether or not anything calls
/// it, so a symbol-presence check answers "the runtime exists", not "the
/// inline allocator was chosen". (The first draft of this file asserted on the
/// bare name and the negative arm failed against a correctly-outlined module.)
const INLINE_SLOW_CALL: &str = "call ptr @js_inline_arena_slow_alloc(";
const INLINE_FAST_BLOCK: &str = "\nalloc.fast";
/// Emitted only by the outlined allocator.
const OUTLINED_CALL: &str = "call i64 @js_object_alloc_class_inline_keys";

const N_ID: u32 = 11;
const WALK_ID: u32 = 700;

fn ir_opts() -> CompileOptions {
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

fn cell_class() -> Class {
    Class {
        id: 3,
        name: "Cell".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            name: "v".to_string(),
            key_expr: None,
            ty: Type::Number,
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
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

/// `function walk(n) { if (n > 0) walk(n - 1); return new Cell(n) }` — the
/// recursive-descent shape, with NO loop anywhere and its entry call in
/// straight-line module init. `recurse = false` drops the self-call, which is
/// the only difference between the two arms.
fn walk_module(recurse: bool) -> Module {
    let mut m = Module::new("alloc_hot.ts");
    m.classes = vec![cell_class()];
    let mut body: Vec<Stmt> = Vec::new();
    if recurse {
        body.push(Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Gt,
                left: Box::new(Expr::LocalGet(N_ID)),
                right: Box::new(Expr::Number(0.0)),
            },
            then_branch: vec![Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::FuncRef(WALK_ID)),
                args: vec![Expr::Binary {
                    op: BinaryOp::Sub,
                    left: Box::new(Expr::LocalGet(N_ID)),
                    right: Box::new(Expr::Number(1.0)),
                }],
                type_args: Vec::new(),
                byte_offset: 0,
            })],
            else_branch: None,
        });
    }
    body.push(Stmt::Return(Some(Expr::New {
        class_name: "Cell".to_string(),
        args: vec![Expr::LocalGet(N_ID)],
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    })));
    m.functions = vec![Function {
        id: WALK_ID,
        name: "walk".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: N_ID,
            name: "n".to_string(),
            ty: Type::Number,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Named("Cell".to_string()),
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    // Straight-line, ONE call site, no loop: the only thing that can admit
    // `walk` is the recursion itself.
    m.init = vec![Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(WALK_ID)),
        args: vec![Expr::Number(8.0)],
        type_args: Vec::new(),
        byte_offset: 0,
    })];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn ir_for(m: Module) -> String {
    String::from_utf8(compile_module(&m, ir_opts()).expect("module compiles"))
        .expect("LLVM IR should be UTF-8")
}

/// Guard against `PERRY_INLINE_NEW`, which forces the inline form everywhere
/// and would make the positive arm pass and the negative arm fail for a reason
/// that has nothing to do with the gate.
fn assert_inline_new_not_forced() {
    assert!(
        std::env::var_os("PERRY_INLINE_NEW").is_none(),
        "these tests describe the DEFAULT gate; PERRY_INLINE_NEW must be unset"
    );
}

#[test]
fn a_self_recursive_function_inlines_its_bump_allocator() {
    assert_inline_new_not_forced();
    let ir = ir_for(walk_module(true));
    assert!(
        ir.contains(INLINE_SLOW_CALL) && ir.contains(INLINE_FAST_BLOCK),
        "`walk` is self-recursive and allocates per level, but its `new` took \
         the outlined allocator — recursion IS a loop, and the lexical test \
         cannot see it:\n{ir}"
    );
    assert!(
        !ir.contains(OUTLINED_CALL),
        "the outlined allocator is still emitted for the recursive function's \
         only `new` site:\n{ir}"
    );
}

/// The anti-bloat half. Identical module minus the self-call: one call site, no
/// loop, not recursive — nothing about it says "runs many times", so it keeps
/// the outlined call and contributes nothing to binary growth.
#[test]
fn a_cold_straight_line_function_keeps_the_outlined_allocator() {
    assert_inline_new_not_forced();
    let ir = ir_for(walk_module(false));
    assert!(
        ir.contains(OUTLINED_CALL),
        "a `new` in a cold, non-recursive, non-in-loop function took the \
         inline bump allocator — the gate has been widened to `always`, which \
         is ~268 bytes per site across the whole program:\n{ir}"
    );
    assert!(
        !ir.contains(INLINE_SLOW_CALL),
        "the inline bump allocator reached a cold site:\n{ir}"
    );
}
