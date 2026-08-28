//! #7871 / #7908: allocation-hot functions and closures take the INLINE bump
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
//! call. Likewise, indirect closure admission has a per-module allocation-site
//! cap. Without those checks this file would pass just as happily if the gate
//! had been widened to "always", which is the ~268-bytes-per-site default the
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
/// Rung 2's outlined entry has an explicit ShapeId argument.
const STAMPED_OUTLINED_CALL: &str = "call i64 @js_object_alloc_class_inline_keys_stamped(";
/// One mint per class at module init, never per allocation.
const SHAPE_MINT_CALL: &str = "call i32 @js_object_shape_id_for_keys(";
/// The immutable id is hoisted to the function-entry setup like keys_array.
const SHAPE_GLOBAL_LOAD: &str = "load i32, ptr @perry_class_shape_id_";
/// #8122: the inline allocator's 16-byte header prefix — packed GcHeader word
/// + `class_id | ShapeId << 32` — is composed ONCE at module init into a
/// per-class `<2 x i64>` global, entry-hoisted like the keys global, and
/// stored with one vector store per allocation.
const HEADER_IMAGE_COMPOSE: &str = "insertelement <2 x i64> <i64 ";
const HEADER_IMAGE_GLOBAL_LOAD: &str = "load <2 x i64>, ptr @perry_class_header_image_";
const HEADER_IMAGE_STORE: &str = "store <2 x i64> %";

const N_ID: u32 = 11;
const WALK_ID: u32 = 700;
const FACTORY_METHOD_ID: u32 = 701;
const STAGE_LOCAL_BASE: u32 = 800;
const STAGE_FUNC_BASE: u32 = 900;

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

fn tiny_factory_class(prefix_stmts: usize) -> Class {
    let mut body = Vec::new();
    for i in 0..prefix_stmts {
        body.push(Stmt::Expr(Expr::Number(i as f64)));
    }
    body.push(Stmt::Return(Some(Expr::New {
        class_name: "Cell".to_string(),
        args: vec![Expr::Number(1.0)],
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    })));
    Class {
        id: 4,
        name: "Factory".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: vec![Function {
            id: FACTORY_METHOD_ID,
            name: "make".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Named("Cell".to_string()),
            body,
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }],
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

fn tiny_factory_module(prefix_stmts: usize) -> Module {
    let mut module = Module::new("tiny_factory.ts");
    module.classes = vec![cell_class(), tiny_factory_class(prefix_stmts)];
    module.init_kind = ModuleInitKind::Eager;
    module
}

fn tiny_factory_budget_module(method_count: u32) -> Module {
    let mut factory = tiny_factory_class(0);
    let template = factory.methods.pop().expect("factory method template");
    factory.methods = (0..method_count)
        .map(|offset| {
            let mut method = template.clone();
            method.id = FACTORY_METHOD_ID + offset;
            method.name = format!("make{offset}");
            method
        })
        .collect();

    let mut module = Module::new("tiny_factory_budget.ts");
    module.classes = vec![cell_class(), factory];
    module.init_kind = ModuleInitKind::Eager;
    module
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

/// A minimized version of #7908's pipeline shape:
///
/// ```text
/// const stage = () => new Cell(1)
/// while (...) stage()
/// ```
///
/// `stage()` is deliberately a `LocalGet`, not a `FuncRef`: the closure may
/// have arrived through an array lookup, so the loop gives codegen no cheap
/// points-to proof. `site_count` creates independent closure allocation sites
/// so the admission budget is observable in emitted IR.
fn indirect_closure_module(site_count: u32, call_in_loop: bool) -> Module {
    let mut m = Module::new("alloc_hot_indirect_closure.ts");
    m.classes = vec![cell_class()];

    for i in 0..site_count {
        m.init.push(Stmt::Let {
            id: STAGE_LOCAL_BASE + i,
            name: format!("stage_{i}"),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Closure {
                func_id: STAGE_FUNC_BASE + i,
                params: Vec::new(),
                return_type: Type::Named("Cell".to_string()),
                body: vec![Stmt::Return(Some(Expr::New {
                    class_name: "Cell".to_string(),
                    args: vec![Expr::Number(i as f64)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                    cap_args_appended: 0,
                }))],
                captures: Vec::new(),
                mutable_captures: Vec::new(),
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: true,
            }),
        });
    }

    let indirect_call = Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::LocalGet(STAGE_LOCAL_BASE)),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    });
    if call_in_loop {
        m.init.push(Stmt::While {
            condition: Expr::Bool(false),
            body: vec![indirect_call],
        });
    } else {
        m.init.push(indirect_call);
    }
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn ir_for(m: Module) -> String {
    String::from_utf8(compile_module(&m, ir_opts()).expect("module compiles"))
        .expect("LLVM IR should be UTF-8")
}

fn ir_for_target(m: Module, target: &str) -> String {
    let mut opts = ir_opts();
    opts.target = Some(target.to_string());
    String::from_utf8(compile_module(&m, opts).expect("module compiles"))
        .expect("LLVM IR should be UTF-8")
}

/// On Apple aarch64 the first-use arena-state resolution reads the hot cache
/// inline (`mrs tpidrro_el0` through the exported pthread key) and keeps the
/// runtime accessor only as the fallback; every other target calls it.
#[test]
fn inline_arena_state_is_resolved_through_the_hot_cache_on_apple_aarch64() {
    assert_inline_new_not_forced();
    if std::env::var_os("PERRY_INLINE_HOT_TLS").as_deref() == Some(std::ffi::OsStr::new("0")) {
        return;
    }
    let ir = ir_for_target(tiny_factory_module(0), "arm64-apple-macosx15.0.0");
    assert!(
        ir.contains("mrs $0, tpidrro_el0")
            && ir.contains("@PERRY_HOT_TSD_KEY")
            && ir.contains("arena_state.hot_tls.ready")
            && ir.contains("call ptr @js_inline_arena_state()"),
        "the Apple aarch64 lowering should read the hot cache inline with the runtime \
         accessor as its fallback:\n{ir}"
    );
    let ir = ir_for_target(tiny_factory_module(0), "x86_64-unknown-linux-gnu");
    assert!(
        !ir.contains("tpidrro_el0") && ir.contains("call ptr @js_inline_arena_state()"),
        "other targets keep the runtime accessor alone:\n{ir}"
    );
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
fn a_tiny_allocation_method_inlines_its_bump_allocator() {
    assert_inline_new_not_forced();
    let ir = ir_for(tiny_factory_module(0));
    assert!(
        ir.contains(INLINE_SLOW_CALL) && ir.contains(INLINE_FAST_BLOCK),
        "a one-statement allocation method is a bounded allocation kernel, but its `new` \
         took the outlined allocator:\n{ir}"
    );
    assert!(
        !ir.contains(STAMPED_OUTLINED_CALL),
        "the tiny allocation method still emitted the outlined allocator:\n{ir}"
    );
}

#[test]
fn a_non_tiny_allocation_method_keeps_the_outlined_allocator() {
    assert_inline_new_not_forced();
    // Two prefix statements plus the return make this a three-statement
    // method, just beyond the deliberately narrow tiny-method admission.
    let ir = ir_for(tiny_factory_module(2));
    assert!(
        ir.contains(STAMPED_OUTLINED_CALL),
        "a three-statement method escaped the bounded tiny-method rule:\n{ir}"
    );
    assert!(
        !ir.contains(INLINE_FAST_BLOCK),
        "the non-tiny method unexpectedly emitted an inline allocation site:\n{ir}"
    );
}

#[test]
fn tiny_allocation_methods_over_the_module_budget_are_all_outlined() {
    assert_inline_new_not_forced();
    let ir = ir_for(tiny_factory_budget_module(9));
    assert!(
        ir.contains(STAMPED_OUTLINED_CALL),
        "allocation methods over the eight-site budget emitted no outlined allocator:\n{ir}"
    );
    assert!(
        !ir.contains(INLINE_FAST_BLOCK),
        "the all-or-none method budget admitted part of a nine-site module:\n{ir}"
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
    assert!(
        ir.contains(SHAPE_MINT_CALL) && ir.contains(HEADER_IMAGE_GLOBAL_LOAD),
        "the inline allocator did not consume the class ShapeId minted at module init \
         (through the module-init header image); newborn instances would keep the \
         allocation-time parent word until a lazy lookup:\n{ir}"
    );
}

/// #8591: the public entry resolves the thread's stable arena state once, and
/// the internal recursive body forwards it through every self call.
#[test]
fn a_self_recursive_allocator_threads_arena_state_through_self_calls() {
    assert_inline_new_not_forced();
    let mut module = walk_module(true);
    module.functions[0].params[0].ty = Type::Any;
    module.functions[0].return_type = Type::Any;
    let ir = ir_for(module);
    assert_eq!(
        ir.matches("call ptr @js_inline_arena_state()").count(),
        1,
        "only the public entry wrapper should resolve arena state:\n{ir}"
    );
    assert!(
        ir.contains("define internal double @perry_fn_alloc_hot_ts__walk.__arena(")
            && ir.contains("ptr %perry_arena_state"),
        "the recursive allocator needs a hidden arena-state body parameter:\n{ir}"
    );
    assert!(
        ir.contains("call double @perry_fn_alloc_hot_ts__walk.__arena(double")
            && ir.contains(", ptr %r"),
        "the public entry and recursive calls must pass the arena-state pointer:\n{ir}"
    );
}

/// #8122: the header prefix must be ONE vector store per allocation, composed
/// ONCE — at module init, into the per-class image global — never two scalar
/// stores whose 40-bit GcHeader constant LLVM rematerialises (`mov` + two
/// `movk`) at every `new`, and not per function either (a recursive allocator
/// like `walk` allocates once per call, so a per-function compose is a
/// per-allocation cost — measured +0.6% on `tree`). So: exactly one compose,
/// in the module init region; the site loads the global and stores the vector.
#[test]
fn the_inline_allocator_stores_its_header_prefix_as_one_vector_image() {
    assert_inline_new_not_forced();
    let ir = ir_for(walk_module(true));
    let compose_count = ir.matches(HEADER_IMAGE_COMPOSE).count();
    assert_eq!(
        compose_count, 1,
        "the header image must be composed exactly once, at module init (found \
         {compose_count} composes):\n{ir}"
    );
    assert!(
        ir.contains(HEADER_IMAGE_GLOBAL_LOAD),
        "the inline allocator must load the module-init header image global:\n{ir}"
    );
    assert!(
        ir.contains(HEADER_IMAGE_STORE),
        "the inline allocation site must store the `<2 x i64>` header image:\n{ir}"
    );
    let merge_at = ir.find("\nalloc.merge").unwrap();
    let merge_end = ir[merge_at + 1..]
        .find("\nshadow.root.barrier")
        .map_or(ir.len(), |at| merge_at + 1 + at);
    let allocation_merge = &ir[merge_at..merge_end];
    assert!(
        !allocation_merge.contains("shl i64 1,") && !allocation_merge.contains("lshr i64"),
        "ordinary inline objects must not pay to update the Map-only object-start bitmap:\n{allocation_merge}"
    );
    // The compose lives beside the ShapeId mint in module init, i.e. after the
    // mint call and outside the allocating function's own body.
    let compose_at = ir.find(HEADER_IMAGE_COMPOSE).unwrap();
    let mint_at = ir.find(SHAPE_MINT_CALL).unwrap();
    assert!(
        compose_at > mint_at,
        "the header image must be composed from the ShapeId the mint returned:\n{ir}"
    );
    // And the per-function fallback compose (from the ShapeId slot) is NOT
    // used when the module-level image exists: no `shl i64 %x, 32` in the
    // allocating function's entry region.
    let fast_at = ir.find(INLINE_FAST_BLOCK).unwrap();
    let fn_start = ir[..fast_at].rfind("\ndefine ").unwrap_or(0);
    let entry_region = &ir[fn_start..fast_at];
    assert!(
        !entry_region.contains(HEADER_IMAGE_COMPOSE),
        "the allocating function composed its own header image although the \
         module-level image global exists:\n{ir}"
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
    assert!(
        !ir.contains(".__arena("),
        "a non-recursive function paid for an arena-threaded entry wrapper:\n{ir}"
    );
    assert!(
        ir.contains(STAMPED_OUTLINED_CALL)
            && ir.contains(SHAPE_MINT_CALL)
            && ir.contains(SHAPE_GLOBAL_LOAD),
        "the cold allocation did not pass its module-init ShapeId to the stamped \
         outlined allocator:\n{ir}"
    );
}

#[test]
fn allocation_closures_are_admitted_by_an_indirect_loop_call() {
    assert_inline_new_not_forced();
    let ir = ir_for(indirect_closure_module(3, true));
    assert!(
        ir.contains(INLINE_SLOW_CALL) && ir.contains(INLINE_FAST_BLOCK),
        "an indirect call in a loop should admit the module's three bounded \
         allocation closures, matching the pipeline stage shape:\n{ir}"
    );
    assert!(
        !ir.contains(OUTLINED_CALL),
        "the admitted closure bodies still use the outlined allocator:\n{ir}"
    );
}

#[test]
fn a_straight_line_indirect_call_does_not_admit_its_closure() {
    assert_inline_new_not_forced();
    let ir = ir_for(indirect_closure_module(1, false));
    assert!(
        ir.contains(OUTLINED_CALL),
        "an indirect call outside a loop supplied no hotness evidence, but its \
         closure allocation was inlined:\n{ir}"
    );
    assert!(
        !ir.contains(INLINE_SLOW_CALL),
        "the inline bump allocator reached a closure with no hot call shape:\n{ir}"
    );
}

#[test]
fn indirect_closure_admission_refuses_modules_over_eight_sites() {
    assert_inline_new_not_forced();
    let ir = ir_for(indirect_closure_module(9, true));
    assert!(
        ir.contains(OUTLINED_CALL),
        "nine closure allocation sites exceed the 8-site / ~2.1 KiB module \
         budget, but the outlined allocator disappeared:\n{ir}"
    );
    assert!(
        !ir.contains(INLINE_SLOW_CALL),
        "an over-budget module admitted some closure sites; admission must be \
         all-or-none so traversal order cannot affect code size:\n{ir}"
    );
}

#[test]
fn indirect_closure_admission_accepts_the_eight_site_budget() {
    assert_inline_new_not_forced();
    let ir = ir_for(indirect_closure_module(8, true));
    assert!(
        ir.contains(INLINE_SLOW_CALL) && ir.contains(INLINE_FAST_BLOCK),
        "eight closure allocation sites are exactly within the module budget, \
         but they did not take the inline allocator:\n{ir}"
    );
    assert!(
        !ir.contains(OUTLINED_CALL),
        "the eight-site boundary was only partially admitted:\n{ir}"
    );
}
