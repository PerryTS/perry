//! #7287: the #5093 class-field versioned loop must actually be REACHED.
//!
//! `lower_class_field_versioned_for` (`stmt/loops.rs`) hoists a monomorphic
//! `this.field` shape check into a loop preheader and runs a guard-free,
//! call-free fast clone. It was written for `benchmarks/suite/09_method_calls.ts`
//! and it is worth ~9× on it. It also matched **nothing** for months, in either
//! configuration, and nothing noticed:
//!
//! * with representation-selection Phase 1 on (the default), a proven-integer
//!   loop counter's *only* storage is its canonical i32 slot — it has no
//!   `ctx.locals` entry — and the matcher gated its counter and its bound on
//!   `ctx.locals.contains_key(..)`;
//! * with Phase 1 off, the counter regains its `ctx.locals` entry but a bare
//!   `i++` counter never earns an i32 *shadow*, which the lowering separately
//!   requires.
//!
//! Every existing signal scored it as working. The lowering compiles, the
//! matcher is exercised by no test, `09_method_calls` still printed the right
//! answer, and the emitted object still differed from an unoptimised build (by
//! the *other* class-field lowerings). Only asserting that the versioned blocks
//! appear in the emitted IR distinguishes "implemented" from "reached" — see
//! CLAUDE.md, "a gate must assert its subject was live".
//!
//! So these tests assert on emitted block labels, and every one of them
//! requires the fast clone AND its guard-free store together: a preheader that
//! is emitted but branched around would still print `class_field.loop.*`.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, Class, ClassField, CompareOp, Expr, Module, ModuleInitKind, Stmt, UpdateOp,
};

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

fn counter_class() -> Class {
    Class {
        id: 101,
        name: "Counter".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            name: "value".to_string(),
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

/// `counter.value = counter.value + 1`, in the shape the inliner leaves behind
/// for `counter.increment()` at module scope: a sloppy `PutValueSet` whose
/// target and receiver are the same local.
fn bump_stmt(recv_id: u32, strict: bool) -> Stmt {
    Stmt::Expr(Expr::PutValueSet {
        target: Box::new(Expr::LocalGet(recv_id)),
        key: Box::new(Expr::String("value".to_string())),
        value: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(recv_id)),
                property: "value".to_string(),
                byte_offset: 0,
            }),
            right: Box::new(Expr::Integer(1)),
        }),
        receiver: Box::new(Expr::LocalGet(recv_id)),
        strict,
    })
}

/// The module-init shape of `benchmarks/suite/09_method_calls.ts` after
/// inlining: `const c = new Counter(); for (let i = 0; i < <bound>; i++) c.value
/// = c.value + 1;`.
fn method_calls_module(bound: Expr, extra_init: Vec<Stmt>, strict: bool) -> Module {
    let mut m = Module::new("class_field_loop.ts");
    m.classes = vec![counter_class()];
    let mut init = extra_init;
    init.push(Stmt::Let {
        id: 1,
        name: "c".to_string(),
        ty: Type::Named("Counter".to_string()),
        mutable: false,
        init: Some(Expr::New {
            class_name: "Counter".to_string(),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        }),
    });
    init.push(Stmt::For {
        init: Some(Box::new(Stmt::Let {
            id: 7,
            name: "i".to_string(),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Integer(0)),
        })),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(7)),
            right: Box::new(bound),
        }),
        update: Some(Expr::Update {
            id: 7,
            op: UpdateOp::Increment,
            prefix: false,
        }),
        body: vec![bump_stmt(1, strict)],
    });
    m.init = init;
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn emit(m: &Module) -> String {
    String::from_utf8(compile_module(m, ir_opts()).unwrap()).expect("LLVM IR should be UTF-8")
}

/// Both halves of the transform, asserted together.
///
/// `class_field.loop.fast.preheader` alone would pass on a lowering that emits
/// the versioned skeleton and then unconditionally branches to the slow clone
/// (which is exactly what `lower_class_field_versioned_for` does when the fast
/// clone turns out not to be call-free). The guard-free store block is the part
/// that only exists when the fast clone was really entered, and the hoisted
/// preheader check is what makes it sound — so require all three.
fn assert_versioned_loop_lowered(ir: &str, what: &str) {
    for label in [
        "class_field.loop.fast.preheader",
        "class_field_loop.preheader.deref",
        "class_field_loop_store.sloppy_fast",
    ] {
        assert!(
            ir.contains(label),
            "{what}: expected the #5093 class-field versioned loop to be lowered, \
             but `{label}` is absent from the emitted IR. The matcher in \
             stmt/loops.rs declined — check that the loop counter and bound are \
             still admitted through `local_has_readable_slot` (repsel Phase 1 \
             stores a proven-integer local ONLY in its canonical i32 slot, with \
             no `ctx.locals` entry). See #7287."
        );
    }
    // The slow clone must survive as the cold arm: it is what a receiver that
    // fails the preheader check (frozen, descriptor-bearing, wrong class) and
    // every mid-loop store side exit falls into.
    assert!(
        ir.contains("for.class_field_slow.cond"),
        "{what}: the versioned loop's SLOW clone is missing — a hoisted guard \
         with no fallback arm is worse than no hoist at all"
    );
    // #7480 step 4: the clone must be ENTERED, not merely emitted. The lowering
    // builds the fast clone first and proves it call-free second; on a failed
    // proof it terminates the guard with an UNCONDITIONAL branch to the slow
    // clone and leaves the fast blocks as unreachable code — a state in which
    // every label assertion above still passes. The twin assertion on the
    // element-shape clone caught exactly that: #7690's back-edge polls put a
    // `js_gc_loop_safepoint()` inside the clone and silently deleted it.
    assert!(
        ir.contains("label %for.class_field_fast.cond")
            || ir.contains("label %class_field.loop.fast.preheader"),
        "{what}: the guard must branch INTO the fast clone. If it ends in an \
         unconditional branch to the slow clone, the call-free proof failed and \
         the clone is dead code that every label assertion above still accepts"
    );
    // The fast clone must be free of the per-access diamond it exists to
    // replace: no volatile gate load between the fast preheader and the store.
    let fast = fast_clone_slice(ir);
    assert!(
        !fast.contains("@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED"),
        "{what}: the fast clone still reads the per-access inline-guard gate; \
         the whole point of the preheader check is that it does not"
    );
    assert!(
        !fast.contains("js_typed_feedback_class_field"),
        "{what}: the fast clone still calls the class-field guard; it must be \
         call-free (call-free ⇒ allocation-free ⇒ no GC ⇒ the preheader's \
         cached object pointer stays valid)"
    );
}

/// The emitted text from the fast clone's condition block up to the slow
/// clone's, i.e. exactly the blocks the fast copy owns.
fn fast_clone_slice(ir: &str) -> &str {
    let start = ir
        .find("for.class_field_fast.cond")
        .expect("fast clone cond block");
    let end = ir[start..]
        .find("for.class_field_slow.cond")
        .map(|off| start + off)
        .unwrap_or(ir.len());
    &ir[start..end]
}

/// The exact `09_method_calls` shape: an integer-literal bound.
#[test]
fn class_field_versioned_loop_fires_for_literal_bound() {
    let ir = emit(&method_calls_module(
        Expr::Integer(10_000_000),
        Vec::new(),
        false,
    ));
    assert_versioned_loop_lowered(&ir, "literal bound");
}

/// The benchmark as actually written: the bound is a module-scope
/// `const ITERATIONS = 10000000`. Under repsel Phase 1 that const is a
/// canonical-i32 local with no `ctx.locals` entry either, so it exercises the
/// bound half of the admission fix independently of the counter half.
#[test]
fn class_field_versioned_loop_fires_for_module_scope_counter() {
    let iterations = Stmt::Let {
        id: 0,
        name: "ITERATIONS".to_string(),
        ty: Type::Number,
        mutable: false,
        init: Some(Expr::Integer(10_000_000)),
    };
    let ir = emit(&method_calls_module(
        Expr::LocalGet(0),
        vec![iterations],
        false,
    ));
    assert_versioned_loop_lowered(&ir, "module-scope const bound");
}

/// STRICT module scope takes a different store lowering
/// (`put_value_static_property_fast_path` → `property_set::lower`), which has
/// carried its own loop-fact branch since #5093. Both arms must reach the fast
/// clone, or an ESM/CJS difference silently changes which one a file gets —
/// the same class of path-dependence #7288 was.
#[test]
fn class_field_versioned_loop_fires_in_strict_mode() {
    let ir = emit(&method_calls_module(
        Expr::Integer(10_000_000),
        Vec::new(),
        true,
    ));
    for label in [
        "class_field.loop.fast.preheader",
        "class_field_loop.preheader.deref",
        "class_field_loop_store.fast",
    ] {
        assert!(
            ir.contains(label),
            "strict mode: expected `{label}` in the emitted IR (#7287)"
        );
    }
    assert!(
        !fast_clone_slice(&ir).contains("@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED"),
        "strict mode: the fast clone still reads the per-access gate"
    );
}
