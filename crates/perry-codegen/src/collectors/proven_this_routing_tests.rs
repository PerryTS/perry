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

/// `class Point { x; y; constructor(x, y); norm2(): number }` — the shape of
/// `benchmarks/repsel_census/fixtures/fixture_ptr_shape.ts`, whose whole purpose
/// is to satisfy every rule in `collectors/ptr_shape.rs` so a local can actually
/// be shape-proven. `norm2` returns `number` and reads only declared fields, so
/// the typed-receiver clone is eligible and its arm runs first — the same
/// shadowing that hid the guarded site, on the other routing site.
fn point_class() -> Class {
    let ctor = func(
        95,
        "constructor",
        vec![param(80, "x", Type::Number), param(81, "y", Type::Number)],
        Type::Void,
        vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "x".to_string(),
                value: Box::new(Expr::LocalGet(80)),
            }),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "y".to_string(),
                value: Box::new(Expr::LocalGet(81)),
            }),
        ],
    );
    let norm2 = func(
        96,
        "norm2",
        Vec::new(),
        Type::Number,
        vec![Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(this_get("x")),
                right: Box::new(this_get("x")),
            }),
            right: Box::new(Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(this_get("y")),
                right: Box::new(this_get("y")),
            }),
        }))],
    );
    let mut c = class(
        102,
        "Point",
        vec![field("x", Type::Number), field("y", Type::Number)],
        vec![norm2],
    );
    c.constructor = Some(ctor);
    c
}

/// A module whose `probe()` holds a Phase 3b **shape-proven local**: a single
/// `Let` initialised by `new` (provenance), only ever field-accessed and
/// method-called, never reassigned, captured, passed, returned or aliased
/// (containment). That combination is what routes through the guard-FREE site
/// in `lower_call/property_get/dynamic_dispatch.rs`, which is a different code
/// path from `guarded_site_module`'s typed parameter.
fn ptr_shape_local_module() -> Module {
    let mut m = Module::new("pshape_local.ts");
    m.classes = vec![point_class()];
    m.functions = vec![func(
        1,
        "probe",
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 3,
                name: "total".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(0.0)),
            },
            Stmt::Let {
                id: 2,
                name: "p".to_string(),
                ty: Type::Named("Point".to_string()),
                mutable: false,
                init: Some(Expr::New {
                    class_name: "Point".to_string(),
                    args: vec![Expr::Number(3.0), Expr::Number(4.0)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                    cap_args_appended: 0,
                }),
            },
            // `p.x = p.x + 1` — a declared-field read and write, which keeps the
            // local inside the proof.
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(2)),
                property: "x".to_string(),
                value: Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(2)),
                        property: "x".to_string(),
                        byte_offset: 0,
                    }),
                    right: Box::new(Expr::Number(1.0)),
                }),
            }),
            // The method result is accumulated into a separate local, exactly as
            // `fixture_ptr_shape.ts` does. Returning `p.norm2()` directly puts a
            // `LocalGet(p)` inside the `Return` expression, which the
            // containment walk treats as an escape — the receiver of a call is
            // not distinguished there — and the local silently stops being
            // shape-proven.
            Stmt::Expr(Expr::LocalSet(
                3,
                Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(call(Expr::LocalGet(2), "norm2", Vec::new())),
                }),
            )),
            Stmt::Return(Some(Expr::LocalGet(3))),
        ],
    )];
    m.init_kind = ModuleInitKind::Eager;
    m
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

/// Regression (#7128), Phase 3b guard-free site: a shape-proven LOCAL whose
/// method also has a typed-receiver clone must still route to the proven-`this`
/// clone.
///
/// `lower_call/property_get/dynamic_dispatch.rs` had the defect in its purest
/// form — the block routed to the clone on its plain exit, but the
/// typed-receiver arm 25 lines above called the guard-ridden public body from
/// its own generic fallback. Both exits are guard-free under the identical
/// Phase 3b proof, so a proven receiver whose ARGUMENTS happened to be
/// non-plain-double silently lost the receiver proof as well.
#[test]
fn ptr_shape_local_typed_fallback_routes_to_proven_this_clone() {
    let ir = emit(&ptr_shape_local_module(), false);
    let defs = pshape_definitions(&ir);
    let calls = pshape_call_targets(&ir);
    let norm2 = defs
        .iter()
        .find(|d| d.contains("__norm2__pshape"))
        .unwrap_or_else(|| {
            panic!(
                "no proven-`this` clone emitted for `norm2` — the Phase 3b \
                 fixture no longer satisfies the shape proof, so this test \
                 would pass vacuously. defined={defs:?}"
            )
        });
    assert!(
        calls.iter().any(|c| c == norm2),
        "a shape-proven local's method call must route to the proven-`this` \
         clone from the guard-free site too. defined={defs:?} called={calls:?}"
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
    let mut ir = emit(&guarded_site_module(), false);
    ir.push('\n');
    ir.push_str(&emit(&ptr_shape_local_module(), false));
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

// ---------------------------------------------------------------------------
// #7142: the class-id dispatch tower, routed under an INLINE keys check.
// ---------------------------------------------------------------------------

/// `class Row { id; weight; score }` with the two methods the tower ratchets
/// need: `rescore` (four declared-field sites — the shape of
/// `benchmarks/app-patterns/kernels/batch.ts`'s hot method) and `tag` (exactly
/// ONE, which is the profitability model's break-even and must be refused).
fn row_class() -> Class {
    let rescore = func(
        97,
        "rescore",
        vec![param(82, "factor", Type::Number)],
        Type::Number,
        vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "score".to_string(),
                value: Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::Binary {
                        op: BinaryOp::Mul,
                        left: Box::new(this_get("weight")),
                        right: Box::new(Expr::LocalGet(82)),
                    }),
                    right: Box::new(this_get("id")),
                }),
            }),
            Stmt::Return(Some(this_get("score"))),
        ],
    );
    let tag = func(
        98,
        "tag",
        Vec::new(),
        Type::Number,
        vec![Stmt::Return(Some(this_get("id")))],
    );
    class(
        103,
        "Row",
        vec![
            field("id", Type::Number),
            field("weight", Type::Number),
            field("score", Type::Number),
        ],
        vec![rescore, tag],
    )
}

/// A module whose `probe(r: Shaped)` calls both `Row` methods on a receiver
/// typed as an INTERFACE. `Shaped` is not in the class registry, which is
/// exactly what `needs_dynamic_dispatch` keys on — so both calls lower to the
/// `idispatch.*` class-id switch tower rather than to either of the two
/// guard-dominated routing sites. This is `batch.ts`'s `rows.map((r) => …)`
/// shape reduced to its essentials.
fn tower_site_module() -> Module {
    let mut m = Module::new("pshape_tower.ts");
    m.classes = vec![row_class()];
    m.functions = vec![func(
        1,
        "probe",
        vec![param(2, "r", Type::Named("Shaped".to_string()))],
        Type::Number,
        vec![Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(call(Expr::LocalGet(2), "rescore", vec![Expr::Number(1.5)])),
            right: Box::new(call(Expr::LocalGet(2), "tag", Vec::new())),
        }))],
    )];
    m.init_kind = ModuleInitKind::Eager;
    m
}

/// Split rendered IR into `(label, body_lines)` — block labels render
/// unindented and colon-terminated, every instruction is indented.
fn blocks(ir: &str) -> Vec<(String, Vec<&str>)> {
    let mut out: Vec<(String, Vec<&str>)> = Vec::new();
    for line in ir.lines() {
        if !line.starts_with(char::is_whitespace)
            && line.ends_with(':')
            && !line.starts_with("define")
        {
            out.push((line.trim_end_matches(':').to_string(), Vec::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push(line);
        }
    }
    out
}

/// The block that contains a `call` to `name`.
fn block_calling<'a>(
    bs: &'a [(String, Vec<&'a str>)],
    name: &str,
) -> Option<&'a (String, Vec<&'a str>)> {
    let needle = format!("@{}(", name);
    bs.iter().find(|(_, body)| {
        body.iter()
            .any(|l| l.contains("call ") && l.contains(&needle))
    })
}

/// Regression (#7142): the class-id dispatch tower routes its case to the
/// proven-`this` clone.
///
/// This is the site #7141 deliberately left alone: `batch.ts`'s receiver has no
/// static class, so neither existing routing site is ever reached and the clone
/// stayed dead on the one workload `Ptr<Shape>` exists for.
#[test]
fn tower_case_routes_to_proven_this_clone() {
    let ir = emit(&tower_site_module(), false);
    // Anti-vacuity: this must really be the class-id tower, not one of the two
    // guard-dominated sites that already routed before this change.
    assert!(
        ir.contains("idispatch.case"),
        "the fixture no longer lowers through the class-id dispatch tower, so \
         this test would pass for the wrong reason:\n{ir}"
    );
    let defs = pshape_definitions(&ir);
    let calls = pshape_call_targets(&ir);
    let rescore = defs
        .iter()
        .find(|d| d.contains("__rescore__pshape"))
        .unwrap_or_else(|| panic!("no proven-`this` clone emitted for `rescore`: {defs:?}"));
    assert!(
        calls.iter().any(|c| c == rescore),
        "the dispatch tower's case proves the receiver's class_id; under an \
         inline keys check it must route to the proven-`this` clone instead of \
         the guard-ridden public body. defined={defs:?} called={calls:?}"
    );
}

/// Soundness ratchet (#7142): the routed call is dominated by a compare of the
/// receiver's live `keys_array` against the class's `@perry_class_keys_*` token.
///
/// A `class_id` match alone is NOT a layout proof — `delete inst.f` compacts
/// the packed slots while preserving `class_id`
/// (`object/delete_rest.rs`), which is why #7141 refused to route this site at
/// all. The compare below is the entire difference between sound and unsound,
/// so it is traced end to end: global → entry-hoisted slot → reload in the
/// guard block → `icmp eq i64` → the branch that enters the clone's block.
#[test]
fn tower_route_is_guarded_by_the_class_keys_token() {
    let ir = emit(&tower_site_module(), false);
    let bs = blocks(&ir);
    let clone = pshape_definitions(&ir)
        .into_iter()
        .find(|d| d.contains("__rescore__pshape"))
        .expect("no proven-`this` clone for `rescore`");
    let (clone_block, _) = block_calling(&bs, &clone)
        .unwrap_or_else(|| panic!("the clone is never called — nothing to guard:\n{ir}"));

    // The block whose terminator enters the clone's block.
    let (_, guard_body) = bs
        .iter()
        .find(|(_, body)| {
            body.iter()
                .any(|l| l.starts_with("  br i1 ") && l.contains(&format!("label %{}", clone_block)))
        })
        .unwrap_or_else(|| {
            panic!("nothing conditionally branches to {clone_block} — the clone is reached unguarded:\n{ir}")
        });

    // 1. the class keys global is loaded once at function entry …
    let global_load = ir
        .lines()
        .find(|l| l.contains("= load i64, ptr @perry_class_keys_"))
        .unwrap_or_else(|| panic!("the class keys token is never read:\n{ir}"));
    let global_reg = global_load.trim().split(' ').next().expect("ssa name");
    // 2. … and parked in an entry slot …
    let store = ir
        .lines()
        .find(|l| l.contains(&format!("store i64 {}, ptr ", global_reg)))
        .unwrap_or_else(|| panic!("the hoisted keys token is never stored:\n{ir}"));
    let slot = store.rsplit(' ').next().expect("slot name");
    // 3. … which the guard block reloads …
    let expected = guard_body
        .iter()
        .find_map(|l| {
            let l = l.trim();
            l.ends_with(&format!("load i64, ptr {}", slot))
                .then(|| l.split(' ').next().expect("ssa name").to_string())
        })
        .unwrap_or_else(|| {
            panic!("the guard block never reads the hoisted keys token:\n{guard_body:#?}")
        });
    // 4. … and compares against the receiver's live keys_array.
    assert!(
        guard_body
            .iter()
            .any(|l| l.contains("icmp eq i64") && l.contains(&expected)),
        "the routed call is not dominated by a keys-token compare — a class_id \
         match alone does not prove the packed layout (`delete` compacts slots \
         while preserving class_id):\n{guard_body:#?}"
    );
    // The sticky prototype-descriptor / tracing latch the per-access inline
    // guard reads must be honoured too, or the route would be weaker than the
    // lowering it replaces.
    assert!(
        guard_body
            .iter()
            .any(|l| l.contains("@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED")),
        "the routed call must also honour the sticky inline-guard latch:\n{guard_body:#?}"
    );
}

/// Profitability ratchet (#7142): a clone that deletes exactly ONE guarded
/// field site does not earn the tower's inline re-check, so the tower keeps
/// calling the public body.
///
/// The re-check IS one instance of the same header check the body would have
/// run at that single site, so routing there swaps a check for a check and adds
/// a second call site for nothing. The clone is still emitted — the two
/// guard-dominated routing sites pay no extra proof and take it happily.
#[test]
fn tower_route_refused_when_clone_deletes_one_field_site() {
    let ir = emit(&tower_site_module(), false);
    let defs = pshape_definitions(&ir);
    let calls = pshape_call_targets(&ir);
    let tag = defs
        .iter()
        .find(|d| d.contains("__tag__pshape"))
        .unwrap_or_else(|| {
            panic!(
                "`tag` admits a proven-`this` clone (it reads a declared field), \
                 so the refusal below would be vacuous without one: {defs:?}"
            )
        });
    assert!(
        !calls.iter().any(|c| c == tag),
        "`tag` has a single guarded field site, so routing the tower to its \
         clone trades one shape check for one shape check plus a branch and a \
         second call site. It must be refused. defined={defs:?} called={calls:?}"
    );
}

// NOTE on the `PERRY_PTR_SHAPE_LOCALS=0` direction: it is deliberately NOT a
// test in this file. `ptr_shape_locals_enabled` memoises in a `OnceLock`, so a
// test that sets the variable in-process observes whatever the first reader in
// the binary cached — it would pass by skipping itself far more often than it
// checked anything, which is precisely the vacuous gate this file exists to
// avoid. The knob direction is exercised out-of-process instead, by the census
// (`benchmarks/repsel_census/README.md`), which re-runs the whole corpus under
// the knob on every CI job and asserts `ptr-shape-consumed` drops to zero.
