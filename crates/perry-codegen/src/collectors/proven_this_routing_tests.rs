//! Phase 5a proven-`this` **call-site** ratchets (#7128).
//!
//! `collectors/proven_this.rs` decides *whether* a `{public}__pshape` clone may
//! be emitted; `codegen/artifacts.rs` emits it. Neither of those is worth
//! anything unless a call site actually *targets* the clone — and for the whole
//! corpus measured in #7128, none did: every clone was emitted, reachable from
//! nothing, and dead-stripped by the linker.
//!
//! The failure was invisible to the two checks that were in place. An object
//! hash A/B scores the phase as working (`suite_09_method_calls`' object DOES
//! differ with the analysis off — by two dead clone bodies), and a promotion
//! counter scores it as working (the clone's `this` is a genuine `Ptr<Shape>`
//! consumption, recorded at every `this.field` site *inside the dead body*).
//! Only reading the emitted IR for a `call` whose callee is the clone
//! distinguishes "routed" from "emitted and abandoned".
//!
//! So these tests assert on **call sites**, never on symbol presence alone:
//! every one of them requires `define`+`call` together, and
//! [`pshape_call_targets`] deliberately matches the callee position so a
//! `ptrtoint ptr @…__pshape` (the shape the typed-feedback guard passes a
//! function pointer in) can never be miscounted as a call.
//!
//! Both routing sites are covered:
//!
//! * `lower_call/method_override.rs` — the guarded `method_direct.fast` arm,
//!   dominated by the class-id + keys-token guard.
//! * `lower_call/property_get/dynamic_dispatch.rs` — the Phase 3b guard-free
//!   `Ptr<Shape>` receiver arm.
//!
//! and in both, the case that regressed is the *typed-clone fallback*: when a
//! typed clone exists for the method, the typed arm ran first and its own
//! generic fallback called the guard-ridden public body, discarding a receiver
//! proof the enclosing block had already established.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Class, ClassField, Expr, Function, Module, ModuleInitKind, Param, Stmt};

fn ir_opts(is_entry: bool) -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: is_entry,
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

fn func(id: u32, name: &str, params: Vec<Param>, return_type: Type, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: name.to_string(),
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
    }
}

fn class(id: u32, name: &str, fields: Vec<ClassField>, methods: Vec<Function>) -> Class {
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
        methods,
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
    }
}

fn this_get(f: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::This),
        property: f.to_string(),
        byte_offset: 0,
    }
}

fn call(recv: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(recv),
            property: method.to_string(),
            byte_offset: 0,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

/// Every LLVM callee named `…__pshape`.
///
/// Matches the **callee position** of a `call` instruction specifically. A
/// `__pshape` symbol also appears in `define` lines and could appear in a
/// `ptrtoint ptr @… to i64` operand (how the typed-feedback guard receives a
/// function pointer); counting either as a call site is exactly the vacuous
/// pass this file exists to prevent.
fn pshape_call_targets(ir: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in ir.lines() {
        let Some(call_at) = line.find("call ") else {
            continue;
        };
        let tail = &line[call_at..];
        // `call <ret-ty> @name(` — the callee is the `@…` immediately before
        // the argument list.
        let Some(at) = tail.find('@') else { continue };
        let Some(paren) = tail[at..].find('(') else {
            continue;
        };
        let name = &tail[at + 1..at + paren];
        if name.ends_with("__pshape") {
            out.push(name.to_string());
        }
    }
    out
}

fn pshape_definitions(ir: &str) -> Vec<String> {
    ir.lines()
        .filter(|l| l.starts_with("define") && l.contains("__pshape"))
        .filter_map(|l| {
            let at = l.find('@')?;
            let paren = l[at..].find('(')?;
            Some(l[at + 1..at + paren].to_string())
        })
        .collect()
}

fn emit(m: &Module, is_entry: bool) -> String {
    String::from_utf8(compile_module(m, ir_opts(is_entry)).unwrap())
        .expect("LLVM IR should be UTF-8")
}

/// `bump(): void { this.value = this.value + 1 }` — a `void` return, so NO
/// typed clone of any tier is eligible (every tier requires a `number`/`i32`/
/// `boolean`/`string` return). This is the arm that already worked.
fn bump_method() -> Function {
    func(
        90,
        "bump",
        Vec::new(),
        Type::Void,
        vec![Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: "value".to_string(),
            value: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(this_get("value")),
                right: Box::new(Expr::Number(1.0)),
            }),
        })],
    )
}

/// `scale(f: number): number { return this.value * f }` — a `number` return
/// and an f64 parameter, so the typed-receiver / typed-f64 clones ARE eligible
/// and their arm runs before the proven-`this` arm. Its generic fallback is
/// what regressed.
fn scale_method() -> Function {
    func(
        91,
        "scale",
        vec![param(70, "f", Type::Number)],
        Type::Number,
        vec![Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Mul,
            left: Box::new(this_get("value")),
            right: Box::new(Expr::LocalGet(70)),
        }))],
    )
}

fn counter_class() -> Class {
    class(
        101,
        "Counter",
        vec![field("value", Type::Number)],
        vec![bump_method(), scale_method()],
    )
}

/// A module whose `probe(c: Counter)` calls both methods on a statically-typed
/// parameter. A typed parameter is not a Phase 3b shape-proven local (no
/// provenance, no containment), so both calls go through the *guarded* site:
/// `emit_guarded_direct_method_call`, behind the class-id + keys-token guard.
fn guarded_site_module() -> Module {
    let mut m = Module::new("pshape_guarded.ts");
    m.classes = vec![counter_class()];
    m.functions = vec![func(
        1,
        "probe",
        vec![param(2, "c", Type::Named("Counter".to_string()))],
        Type::Void,
        vec![
            Stmt::Expr(call(Expr::LocalGet(2), "bump", Vec::new())),
            Stmt::Expr(call(Expr::LocalGet(2), "scale", vec![Expr::Number(1.5)])),
        ],
    )];
    m.init_kind = ModuleInitKind::Eager;
    m
}

/// Regression: a method with NO eligible typed clone routes to its clone.
///
/// This half was already true before #7128 and is kept as the control — if it
/// ever goes red, the routing site itself (not the arm ordering) has broken.
#[test]
fn untyped_method_routes_to_proven_this_clone() {
    let ir = emit(&guarded_site_module(), false);
    let defs = pshape_definitions(&ir);
    let calls = pshape_call_targets(&ir);
    let bump = defs
        .iter()
        .find(|d| d.contains("__bump__pshape"))
        .unwrap_or_else(|| panic!("no proven-`this` clone emitted for `bump`: {defs:?}"));
    assert!(
        calls.iter().any(|c| c == bump),
        "`bump`'s proven-`this` clone is emitted but nothing calls it — it will \
         be dead-stripped. defined={defs:?} called={calls:?}"
    );
}

/// Regression (#7128): a method that ALSO has a typed clone must still route
/// its generic fallback to the proven-`this` clone.
///
/// Before the fix, `emit_guarded_direct_method_call` tried the five typed arms
/// first and only the final `else` consulted `pshape_methods`. Every method
/// that admits a proven-`this` clone must touch a declared field of its own
/// chain — which is very nearly the definition of a typed-receiver-clone
/// candidate — so the typed arm won essentially whenever both were eligible,
/// and the clone was emitted with no caller at all.
///
/// The typed arm's fast path is deliberately left alone (that is a
/// cost-model question, tracked separately): what must be true is that the
/// fallback it branches to no longer throws the receiver proof away.
#[test]
fn typed_clone_fallback_routes_to_proven_this_clone() {
    let ir = emit(&guarded_site_module(), false);
    let defs = pshape_definitions(&ir);
    let calls = pshape_call_targets(&ir);
    let scale = defs
        .iter()
        .find(|d| d.contains("__scale__pshape"))
        .unwrap_or_else(|| panic!("no proven-`this` clone emitted for `scale`: defined={defs:?}"));
    assert!(
        calls.iter().any(|c| c == scale),
        "`scale` has a typed clone, so the typed arm runs first; its generic \
         fallback must still route to the proven-`this` clone instead of the \
         guard-ridden public body. defined={defs:?} called={calls:?}"
    );
    // The typed arm itself must survive — this fix reroutes the FALLBACK, it
    // does not displace the typed clone.
    assert!(
        ir.contains("__typed_f64_recv") || ir.contains("__typed_f64"),
        "the typed clone must still be emitted and preferred on the fast \
         path:\n{ir}"
    );
}

/// The routed call must still hand the callee a shadow-bound receiver slot.
///
/// #6925 kept the clone's `(double this, …)` ABI and its shadow-bound,
/// tagged-at-rest receiver slot precisely because `GC_TYPE_OBJECT` is MOVABLE
/// in the shipped configuration (#7019) — the `TaPtr` no-bind shortcut does not
/// transfer. Routing more call sites to the clone is only safe while that
/// remains true, so assert it at the callee rather than trusting the comment.
#[test]
fn proven_this_clone_binds_its_receiver_slot() {
    let ir = emit(&guarded_site_module(), false);
    let names = pshape_definitions(&ir);
    assert!(
        !names.is_empty(),
        "nothing to check — no proven-`this` clone was emitted at all"
    );
    for name in names {
        // Anchor on the DEFINITION line, not the first mention: a routed call
        // site names the same symbol and appears earlier in the module, so
        // `find("@{name}(")` alone would slice the caller's body and then
        // "pass" by finding some other function's receiver bind.
        let def_line = ir
            .lines()
            .position(|l| l.starts_with("define") && l.contains(&format!("@{name}(")))
            .unwrap_or_else(|| panic!("clone {name} has no definition line"));
        let body: String = ir
            .lines()
            .skip(def_line)
            .take_while(|l| *l != "}")
            .collect::<Vec<_>>()
            .join("\n");
        let body = body.as_str();
        let store = body
            .find("store double %this_arg")
            .unwrap_or_else(|| panic!("{name}: receiver is never stored to a slot:\n{body}"));
        let bind = body
            .find("call void @js_shadow_slot_bind")
            .unwrap_or_else(|| panic!("{name}: receiver slot is never shadow-bound:\n{body}"));
        assert!(
            store < bind,
            "{name}: the receiver must be stored to its slot BEFORE the slot is \
             bound, with no safepoint between:\n{body}"
        );
    }
}

// NOTE on the `PERRY_PTR_SHAPE_LOCALS=0` direction: it is deliberately NOT a
// test in this file. `ptr_shape_locals_enabled` memoises in a `OnceLock`, so a
// test that sets the variable in-process observes whatever the first reader in
// the binary cached — it would pass by skipping itself far more often than it
// checked anything, which is precisely the vacuous gate this file exists to
// avoid. The knob direction is exercised out-of-process instead, by the census
// (`benchmarks/repsel_census/README.md`), which re-runs the whole corpus under
// the knob on every CI job and asserts `ptr-shape-consumed` drops to zero.
