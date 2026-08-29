//! #7622 — the same HIR module must emit byte-identical LLVM IR every time.
//!
//! Two emission sites read their order straight out of a `std::collections`
//! hash map. Rust's default hasher is seeded per `RandomState`, so the order
//! was a fresh permutation on every compile: the same source, compiled twice by
//! the same `perry` binary, produced different `.ll`.
//!
//! * **Function-name registration** (`codegen/artifacts.rs`). Every inline
//!   closure carrying a HIR display name mints a rodata constant through
//!   `add_string_constant` — whose `@.str.N` counter numbers in first-use order
//!   — and emits one `js_register_function_name` call into
//!   `__perry_init_strings_*`. Iterating `hir.closure_display_names` permuted
//!   both. (#7038 fixed the identical defect in the `closure_source_text` loop
//!   directly below it and left this one standing.)
//! * **The dynamic method-dispatch tower**
//!   (`lower_call/property_get/dynamic_dispatch.rs`). Every class implementing
//!   the called property becomes one `icmp`-guarded case block; iterating
//!   `ctx.class_ids` permuted the arms, so consecutive call sites named
//!   different `perry_method_*` callees run to run.
//!
//! That is not cosmetic. It defeats the byte-level IR A/B that the #7615
//! rooting-migration slices offer as their correctness evidence (three of nine
//! apparent diffs in #7620 were this, not the change), and the `.perry-cache`
//! object cache keys on a DETERMINISTIC fingerprint — so a cache hit and a
//! rebuild can legitimately hold different bytes for the same inputs.
//!
//! ## Why these tests are not vacuous
//!
//! Each case builds its `Module` FRESH for every compile. That matters: the
//! offending maps live in the HIR, so compiling one long-lived `Module` twice
//! would iterate the very same `RandomState` twice and pass no matter what.
//! `N = 16` entries makes a chance-ordered agreement a 1-in-16! event.
//!
//! Each case also asserts its subject was LIVE before it judges order — the
//! registration count, and the tower arm count. A fixture that stopped emitting
//! the construct under test would otherwise go green having proven nothing.
//!
//! Sabotage-verified: reverting either sort in isolation turns exactly that
//! shape's `…_are_emitted_in_…_order` and `…_is_run_to_run_deterministic`
//! tests red, and leaves the other shape's green.
//!
//! ## What is NOT covered here, and why
//!
//! The same commit sorts two further hash-order reads that this file does not
//! test, because no fixture was found that makes them emit anything:
//!
//! * the **virtual-override tower** (`vdispatch.*`, the sibling of the tower
//!   above, in the same file, over the same map). `vdispatch` blocks appear in
//!   ZERO of 41 sampled `test_gap_*` programs — every receiver-typed call
//!   measured is claimed first by `method_override.rs`'s `method_direct` shape
//!   guard. A green test over a fixture that emits no arms would assert
//!   nothing, which is worse than no test, so there is none.
//! * `method_registry.rs`'s `class_table` walk, whose `insert` (last wins) and
//!   `entry().or_insert_with` (first wins) tie-breaks only diverge when two
//!   distinct `&Class` contend for one registry key.
//!
//! Both are mechanical siblings of the defects that ARE covered — and #7622
//! exists precisely because #7038 fixed one such loop and left its neighbour —
//! so they are sorted, and labelled untested rather than left implied.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, Expr, Function, Module, ModuleInitKind, Param, Stmt};

/// Enough entries that an accidentally-sorted hash order is not a plausible
/// explanation for a green run.
const N: u32 = 16;

fn ir_opts() -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: false,
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

fn empty_module(name: &str) -> Module {
    Module {
        name: name.to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: Vec::new(),
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        init: Vec::new(),
        classic_for_lexical_bindings: std::collections::HashSet::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
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

fn ir(module: &Module) -> String {
    String::from_utf8(compile_module(module, ir_opts()).expect("codegen should succeed"))
        .expect("LLVM IR should be UTF-8")
}

// ---------------------------------------------------------------------------
// Shape 1: `js_register_function_name` / `@.str.N`
// ---------------------------------------------------------------------------

/// `let _fK = () => {}` for `K` in `0..N`, each carrying a HIR display name.
///
/// The `_` prefix is load-bearing: the top-level `let`-bound arm of the
/// display-name collection skips underscore names outright, so every entry
/// falls through to the arm that reads `hir.closure_display_names` — the one
/// under test.
fn closure_display_module() -> Module {
    let mut m = empty_module("emission_order_names.ts");
    for k in 0..N {
        let func_id = 100 + k;
        m.init.push(Stmt::Let {
            id: 900 + k,
            name: format!("_f{:02}", k),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Closure {
                func_id,
                params: Vec::new(),
                return_type: Type::Void,
                body: Vec::new(),
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
        m.closure_display_names
            .insert(func_id, format!("name{:02}", k));
    }
    m
}

/// The `func_id` of every `js_register_function_name` call, in emission order.
fn registered_closure_ids(ir: &str) -> Vec<u32> {
    ir.lines()
        .filter(|l| l.contains("call void @js_register_function_name("))
        .filter_map(|l| {
            let at = l.find("@perry_closure_")?;
            let rest = &l[at..];
            let end = rest.find(',')?;
            rest[..end].rsplit("__").next()?.parse::<u32>().ok()
        })
        .collect()
}

#[test]
fn closure_display_names_are_emitted_in_func_id_order() {
    let ids = registered_closure_ids(&ir(&closure_display_module()));
    // Liveness: the construct under test was actually emitted.
    assert_eq!(
        ids.len(),
        N as usize,
        "expected one js_register_function_name per closure display name; \
         the fixture stopped exercising the emission path"
    );
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(
        ids, sorted,
        "js_register_function_name calls must be emitted in FuncId order, not \
         `hir.closure_display_names` hash order (#7622)"
    );
}

#[test]
fn closure_display_name_emission_is_run_to_run_deterministic() {
    // A FRESH module per compile — same content, a different `RandomState` for
    // its `closure_display_names` map. Reusing one module would re-iterate the
    // same map and pass unconditionally.
    let first = ir(&closure_display_module());
    let second = ir(&closure_display_module());
    assert!(
        first.contains("call void @js_register_function_name("),
        "liveness: fixture emitted no function-name registrations"
    );
    assert_eq!(
        first, second,
        "two compiles of the same module must emit byte-identical IR (#7622)"
    );
}

// ---------------------------------------------------------------------------
// Shape 2: the class-id dispatch tower
// ---------------------------------------------------------------------------

fn method_fn(id: u32, name: &str) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::Number(1.0)))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    }
}

fn plain_class(id: u32, name: &str, method: Function) -> Class {
    Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: vec![method],
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

/// `N` classes that all declare `m()`, plus `function callm(o: any) { o.m(); }`.
///
/// `o` is an `any`-typed parameter, so `receiver_class_name` cannot name a
/// class and the call lowers through the dynamic-dispatch tower — one
/// `icmp`-guarded case per implementing class.
///
/// Class ids run OPPOSITE to class names on purpose: `C00` gets id `N`, `C15`
/// gets id 1. The emission order under test is by class id, and with ids
/// assigned in name order the two are indistinguishable — a sort keyed on the
/// class NAME would satisfy the assertion just as well, so the test would not
/// pin the property it claims. Reversed, only a class-id sort produces the
/// expected sequence.
fn dispatch_tower_module() -> Module {
    let mut m = empty_module("emission_order_tower.ts");
    for k in 0..N {
        m.classes.push(plain_class(
            N - k,
            &format!("C{:02}", k),
            method_fn(200 + k, "m"),
        ));
    }
    m.functions.push(Function {
        id: 700,
        name: "callm".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: 701,
            name: "o".to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Void,
        body: vec![Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(701)),
                property: "m".to_string(),
                byte_offset: 0,
            }),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        })],
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    });
    m
}

/// The class name of every `call … @perry_method_*__m(...)` inside the body of
/// `callm` — i.e. the dispatch tower's arm order.
///
/// Scoped to that one function on purpose. Codegen also emits a per-class
/// closure-call wrapper (`artifacts.rs`, one `call @perry_method_…__m` each),
/// and those walk the `hir.classes` Vec, so they are already ordered and would
/// mask the tower's order if folded into the same list.
fn tower_arm_classes(ir: &str) -> Vec<String> {
    let mut in_callm = false;
    let mut out = Vec::new();
    for l in ir.lines() {
        if l.starts_with("define ") {
            in_callm = l.contains("callm");
            continue;
        }
        if !in_callm || !l.contains("call") || !l.contains("@perry_method_") {
            continue;
        }
        let Some(at) = l.find("@perry_method_") else {
            continue;
        };
        let rest = &l[at..];
        let Some(end) = rest.find('(') else { continue };
        // `@perry_method_<modprefix>__<Class>__m`
        let Some(class) = rest[..end]
            .strip_suffix("__m")
            .and_then(|s| s.rsplit("__").next())
        else {
            continue;
        };
        if class.starts_with('C') {
            out.push(class.to_string());
        }
    }
    out
}

#[test]
fn dispatch_tower_arms_are_emitted_in_class_id_order() {
    let arms = tower_arm_classes(&ir(&dispatch_tower_module()));
    // Liveness: the tower was actually emitted, with one arm per class. Without
    // this the test would pass on a fixture that stopped producing a tower at
    // all (e.g. if the receiver ever became statically typed).
    assert_eq!(
        arms.len(),
        N as usize,
        "expected one dispatch-tower arm per implementing class, got {:?}",
        arms
    );
    // Spelled out rather than derived by sorting `arms` itself: `C{:02}` runs
    // opposite to the class ids, so this sequence is satisfied ONLY by a
    // class-id ordering. A name-keyed sort — or `arms.sort()` compared against
    // itself — would accept the reverse and prove nothing.
    let expected: Vec<String> = (0..N).rev().map(|k| format!("C{:02}", k)).collect();
    assert_eq!(
        arms, expected,
        "dispatch-tower arms must be emitted in class-id order, not \
         `ctx.class_ids` hash order (#7622)"
    );
}

#[test]
fn dispatch_tower_emission_is_run_to_run_deterministic() {
    let first = ir(&dispatch_tower_module());
    let second = ir(&dispatch_tower_module());
    assert!(
        first.contains("@perry_method_"),
        "liveness: fixture emitted no class methods"
    );
    assert_eq!(
        first, second,
        "two compiles of the same module must emit byte-identical IR (#7622)"
    );
}
