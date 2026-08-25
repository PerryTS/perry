//! #7521: `Stmt::PreallocateBoxes` must not shadow a module-level global.
//!
//! A module-level binding that any function or closure reads is promoted to
//! `@perry_global_<mod>__<id>` (`codegen/module_globals_emit.rs`) — a single,
//! shared, GC-rooted, forward-visible cell. That is *also* what a prealloc box
//! provides, so the two mechanisms are alternatives, and every read/write site
//! in codegen already spells out which one wins:
//!
//! ```text
//! ctx.boxed_vars.contains(id) && !ctx.module_globals.contains_key(id)
//! ```
//!
//! `emit_preallocate_boxes` was the one place that did not check. Allocating a
//! box for a promoted id is not merely redundant: it registers a `ctx.locals`
//! slot, and `ctx.locals` is consulted BEFORE `ctx.module_globals` on the
//! `Stmt::Let` reuse path (`let_stmt.rs`) and on the `LocalGet`/`LocalSet`
//! store paths (`expr/literals_vars.rs`). The declaration then writes its value
//! into the box-pointer slot, the global is never stored, and every closure
//! that reads the binding through the global sees the `undefined` it was
//! defined with.
//!
//! The user-visible shape (test-files/test_gap_diagchannel_3082_3084_3085_3086.ts,
//! and any ES module at all after #7105 started emitting `PreallocateBoxes` for
//! module-top-level blocks):
//!
//! ```js
//! { const events = []; function t() { events.push("x"); } t(); events.length }
//! ```
//!
//! `t()` ran, `events.length` was 0, and nothing threw — the push landed in a
//! cell nobody else read.
//!
//! The assertions below are on emitted IR rather than on a flag, because the
//! bug is precisely that a live, correct-looking box was built for a binding
//! that stores somewhere else (CLAUDE.md: "a gate must assert its subject was
//! live").

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Module, ModuleInitKind, Param, Stmt};

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

fn emit(m: &Module) -> String {
    String::from_utf8(compile_module(m, ir_opts()).unwrap()).expect("LLVM IR should be UTF-8")
}

/// `function t(a) { events = a; }` — the hoisted block-level function
/// declaration, as `lower_strict_block_fn_decls` leaves it: a `Stmt::Let`
/// bound to a closure that both reads and writes the block's `const`, placed
/// AHEAD of that const's own declaration.
fn hoisted_writer(fn_local: u32, target: u32) -> Stmt {
    Stmt::Let {
        id: fn_local,
        name: "t".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Closure {
            func_id: 1,
            params: vec![Param {
                id: 40,
                name: "a".to_string(),
                ty: Type::Any,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            }],
            return_type: Type::Any,
            body: vec![Stmt::Expr(Expr::LocalSet(
                target,
                Box::new(Expr::LocalGet(40)),
            ))],
            captures: vec![target],
            mutable_captures: vec![target],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: true,
        }),
    }
}

/// The exact module-init shape `lower_strict_block_fn_decls` produces for
/// `{ const events = []; function t(a) { events = a } t(1) }` at the top level
/// of an ES module: the prealloc directive, then the hoisted closure, then the
/// binding's own declaration.
///
/// `prealloc` is a parameter so the "with" and "without" arms below are the
/// same fixture, differing only in the statement under test.
fn module_with_hoisted_block_fn(prealloc: bool) -> Module {
    let mut m = Module::new("prealloc_global.ts");
    let mut init = Vec::new();
    if prealloc {
        init.push(Stmt::PreallocateBoxes(vec![1]));
    }
    init.push(hoisted_writer(0, 1));
    init.push(Stmt::Let {
        id: 1,
        name: "events".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Array(Vec::new())),
    });
    init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::LocalGet(0)),
        args: vec![Expr::Integer(1)],
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    m.init = init;
    m.init_kind = ModuleInitKind::Eager;
    m
}

/// The slice of `ir` that is the entry module's init function (`main`), which
/// is where the binding's own `Stmt::Let` is emitted. Deliberately excludes the
/// closure body — the closure ALWAYS stored to the global, before and after the
/// fix, so an assertion over the whole module would have been green throughout.
fn main_body(ir: &str) -> &str {
    let start = ir
        .find("define i32 @main()")
        .expect("entry module must emit main()");
    let rest = &ir[start..];
    let end = rest.find("\n}\n").map(|e| e + start).unwrap_or(ir.len());
    &ir[start..end]
}

const GLOBAL: &str = "@perry_global_prealloc_global_ts__1";

/// Count the STORES to the module global in `body`. A plain `contains` is not
/// enough: `main` also takes the global's address for
/// `js_gc_register_global_root`, and that reference is emitted whether or not
/// the declaration ever writes the cell — it kept the pre-fix IR looking fine.
fn stores_to_global(body: &str) -> usize {
    body.lines()
        .filter(|l| {
            let l = l.trim();
            l.starts_with("store ") && l.ends_with(&format!("ptr {GLOBAL}"))
        })
        .count()
}

/// Fixture premise. If `events` ever stops being promoted, the tests below
/// would pass vacuously — they would be asserting about a global that no longer
/// participates.
#[test]
fn the_captured_module_binding_is_promoted_to_a_global() {
    for prealloc in [false, true] {
        let ir = emit(&module_with_hoisted_block_fn(prealloc));
        assert!(
            ir.contains(&format!("{GLOBAL} = ")),
            "premise (prealloc={prealloc}): a module-level binding read from a \
             closure must be promoted to a module global"
        );
    }
}

/// The regression. `PreallocateBoxes` must not divert the declaration's store
/// away from the shared cell.
///
/// Sabotage: delete the `ctx.module_globals.contains_key(id)` early-continue in
/// `emit_preallocate_boxes` (`stmt/mod.rs`) and this goes red — the box-pointer
/// slot swallows the store, `main` never touches the global, and every closure
/// reading it sees `undefined`. That is #7521.
#[test]
fn a_preallocated_box_does_not_swallow_the_module_global_store() {
    let ir = emit(&module_with_hoisted_block_fn(true));
    let main = main_body(&ir);
    assert!(
        stores_to_global(main) > 0,
        "#7521: the declaration `const events = []` must store into the shared \
         module global, but main() never references {GLOBAL} as a store target. \
         `emit_preallocate_boxes` registered a ctx.locals slot for a promoted \
         id, and ctx.locals is consulted before ctx.module_globals on the \
         Stmt::Let / LocalSet paths.\n--- main() ---\n{main}"
    );
}

/// The prealloc arm must emit exactly what the no-prealloc arm emits for this
/// binding: the directive is a no-op for a promoted id, not a different
/// lowering that happens to also store the global.
#[test]
fn prealloc_is_a_no_op_for_a_promoted_binding() {
    let with = emit(&module_with_hoisted_block_fn(true));
    let without = emit(&module_with_hoisted_block_fn(false));
    let count = |ir: &str| stores_to_global(main_body(ir));
    assert_eq!(
        count(&with),
        count(&without),
        "#7521: main() must STORE the module global the same number of times \
         with and without PreallocateBoxes"
    );
    assert!(
        !main_body(&with).contains("js_box_alloc_bits"),
        "#7521: no box may be allocated in main() for a binding whose storage \
         is already a module global\n--- main() ---\n{}",
        main_body(&with)
    );
}

/// The fix must not gut #569 / #6044. A prealloc'd id that is NOT a module
/// global — the ordinary case, a `let`/`const` inside a function body captured
/// by a hoisted sibling `function` — still needs its box.
#[test]
fn a_function_local_prealloc_still_gets_its_box() {
    let mut m = Module::new("prealloc_local.ts");
    // An arrow IIFE at module scope: its body's ids are function-locals, so
    // nothing inside is eligible for module-global promotion.
    let inner = vec![
        Stmt::PreallocateBoxes(vec![11]),
        hoisted_writer(10, 11),
        Stmt::Let {
            id: 11,
            name: "events".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Array(Vec::new())),
        },
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(10)),
            args: vec![Expr::Integer(1)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];
    m.init = vec![Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::Closure {
            func_id: 2,
            params: Vec::new(),
            return_type: Type::Any,
            body: inner,
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
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    })];
    m.init_kind = ModuleInitKind::Eager;
    let ir = emit(&m);
    assert!(
        ir.contains("js_box_alloc_bits"),
        "#569/#6044: a prealloc'd id with no module global must still get a \
         heap box — the #7521 guard is scoped to promoted ids only"
    );
}
