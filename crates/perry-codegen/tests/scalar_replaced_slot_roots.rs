//! #6968: a heap value stored into a scalar-replaced object field or array
//! element must be a precise GC root.
//!
//! Scalar replacement deletes the object and keeps one entry-block alloca per
//! field. Those allocas belong to no HIR local, so the pre-lowering
//! `collect_pointer_typed_locals` pass — which assigns shadow slots by walking
//! `Stmt::Let` — cannot see them, and nothing bound them. A collection landing
//! between the store and the read swept the value out from under the alloca.
//!
//! The end-to-end proof is `test-files/test_gap_repsel_scalar_replaced_locals.ts`
//! on the `cons_scan_off` arm of `scripts/gc_repsel_matrix.sh` — the only
//! configuration where the bug is observable, because every automatic
//! collection otherwise forces a conservative native-stack scan that pins the
//! alloca by accident. These tests pin the *codegen contract* that arm depends
//! on, in-process, so a lowering path that goes back to emitting a bare alloca
//! fails here instead of under a later narrowing of the forced scan.
//!
//! Both directions are covered: the gate must stay silent for a
//! literal whose fields are numbers, or every scalar-replaced `{x, y}` in a hot
//! loop would pay for rooting a value that can never be collected (#6997).
//!
//! LOWERING (#7493): every test here measures `js_shadow_slot_bind` /
//! `js_shadow_frame_enter` call sites, which only the SHADOW-STACK lowering
//! emits — so every test pins `NativeRootsPin::shadow()`. Native roots express
//! the same root set as `ptr addrspace(1)` allocas that LLVM's RS4GC pass
//! relocates; there is no bind call to count.
//!
//! That distinction was not cosmetic for the two `numeric_only_*_emits_no_rooting`
//! tests. They assert no rooting is emitted, and under the post-#7370 default
//! the bind count is zero for EVERY program, rooted or not — so they were
//! passing vacuously, which is CLAUDE.md hazard 4 ("the gate runs but its
//! subject never did"). Pinning them made them assert their subject again.
//!
//! ATTRIBUTION (#7504): pinning then exposed a second defect one layer down.
//! The measurement was a WHOLE-MODULE counter — `ir.matches("call void
//! @js_shadow_slot_bind(").count()` — and since #7487 a pooled temp root emits
//! the identical call. Every fixture in this file ends in
//! `console.log(o.a, o.b)`, whose argument accumulator contributes three binds
//! naming an alloca that has nothing to do with any scalar-replaced field. So
//! `== 0` was a claim about the accumulator and `== 1` a coincidence.
//!
//! Every count here now goes through `perry_codegen::testing::root_slots`,
//! which keys each bind and each root-shading barrier by the entry alloca it
//! names and refuses to classify an unfamiliar one. Where a fixture's temp-root
//! traffic is non-zero the tests SAY so, because that is what proves the reader
//! was looking at a module in which binds exist at all.
//!
//! The native-roots side of this contract — that a scalar-replaced field
//! holding a heap value becomes a relocatable `addrspace(1)` slot, and a
//! numeric one does not — is covered by
//! `perry_codegen::native_root_coverage::mechanics` since #7653.

use perry_codegen::testing::root_slots::{
    frame_slot_count, temp_root_slot_binds, value_slot_barriers, value_slot_binds,
};
use perry_codegen::testing::NativeRootsPin;
use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Module, ModuleInitKind, Stmt};

fn entry_opts() -> CompileOptions {
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
        fp_contract_mode: perry_codegen::FpContractMode::Off,
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

fn module_with_init(name: &str, init: Vec<Stmt>) -> Module {
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
        init,
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
        class_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        local_source_spans: std::collections::HashMap::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

fn ir_for(name: &str, init: Vec<Stmt>) -> String {
    String::from_utf8(compile_module(&module_with_init(name, init), entry_opts()).unwrap())
        .expect("LLVM IR should be UTF-8")
}

fn let_stmt(id: u32, name: &str, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

/// A heap value with no other reference: a fresh object literal that is not
/// itself bound to a local, so it is allocated on the heap and the field
/// alloca is the only thing pointing at it.
fn heap_value() -> Expr {
    Expr::Object(vec![("k".to_string(), Expr::Number(1.0))])
}

fn field_get(local: u32, field: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(local)),
        property: field.to_string(),
        byte_offset: 0,
    }
}

fn console_log(args: Vec<Expr>) -> Stmt {
    Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::GlobalGet(0)),
            property: "log".to_string(),
            byte_offset: 0,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    })
}

/// The `main` these entry-module fixtures compile into.
///
/// Every assertion below is scoped to one function rather than to the module,
/// so a bind emitted in an unrelated helper cannot satisfy or break a claim
/// about this one.
fn main_ir(ir: &str) -> &str {
    perry_codegen::testing::root_slots::function_slice(ir, "main")
}

/// A scalar-replaced object literal whose field holds a heap value must bind
/// that field's alloca as a precise root.
///
/// Pre-fix the field store was `store double %v, ptr %slot` into a bare
/// entry-block alloca with no `js_shadow_slot_bind` anywhere in the function:
/// the object local's own reserved slot is only ever *cleared*, because scalar
/// replacement leaves no object handle to bind. A collection between the store
/// and the read therefore swept the value (#6968).
#[test]
fn scalar_replaced_object_field_holding_a_heap_value_is_bound() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_for(
        "scalar_object_field_root.ts",
        vec![
            let_stmt(
                1,
                "o",
                Expr::Object(vec![
                    ("a".to_string(), heap_value()),
                    ("b".to_string(), Expr::Number(2.0)),
                ]),
            ),
            console_log(vec![field_get(1, "a"), field_get(1, "b")]),
        ],
    );

    assert!(
        value_slot_binds(main_ir(&ir)) > 0,
        "the scalar-replaced field alloca holding a heap value must be bound \
         as a precise root (#6968):\n{ir}"
    );

    // Frame growth, stated DIFFERENTIALLY. `frame_slot_count(&ir) > 0` on its
    // own certifies nothing: `o` is pointer-typed, so the pre-lowering pass
    // already reserved it a slot and the frame is non-empty with or without
    // this fix. The claim that has teeth is that the pointer-capable field
    // takes an ADDITIONAL slot the pointer analysis could not have predicted,
    // so compare against the structurally identical numeric-only literal —
    // same local, same field count, same reads, no rooting.
    let control = ir_for(
        "scalar_object_field_root_control.ts",
        vec![
            let_stmt(
                1,
                "o",
                Expr::Object(vec![
                    ("a".to_string(), Expr::Number(1.0)),
                    ("b".to_string(), Expr::Number(2.0)),
                ]),
            ),
            console_log(vec![field_get(1, "a"), field_get(1, "b")]),
        ],
    );
    assert!(
        frame_slot_count(main_ir(&ir)) > frame_slot_count(main_ir(&control)),
        "binding a scalar-replacement alloca must grow the shadow frame beyond \
         what the pre-lowering pointer analysis reserved: heap-field literal \
         has {} slots, the numeric-only control has {} — the pre-lowering pass \
         cannot see these allocas, so the extra slot can only come from \
         `reserve_shadow_slot` (#6968):\n{ir}",
        frame_slot_count(main_ir(&ir)),
        frame_slot_count(main_ir(&control)),
    );
}

/// The gate, from the other side: a literal whose every field is a number
/// must emit no rooting at all.
///
/// This is the #6997 lesson — rooting a value that can never be collected is
/// pure cost on the path that exists *because* it was optimized. The decision
/// is made from the lowering (`expr_is_known_non_pointer_shadow_value`), not
/// from a declared type, so it holds for `any`-typed locals too — which is
/// exactly what this module builds (`Type::Any`).
#[test]
fn numeric_only_scalar_replaced_object_emits_no_rooting() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_for(
        "scalar_object_numeric.ts",
        vec![
            let_stmt(
                1,
                "p",
                Expr::Object(vec![
                    ("x".to_string(), Expr::Number(1.0)),
                    ("y".to_string(), Expr::Number(2.0)),
                ]),
            ),
            console_log(vec![field_get(1, "x"), field_get(1, "y")]),
        ],
    );

    let main = main_ir(&ir);
    assert_eq!(
        value_slot_binds(main),
        0,
        "a scalar-replaced literal with only numeric fields must not pay for \
         GC rooting:\n{main}"
    );
    // Non-vacuity, in the same test: this module DOES bind — three times, for
    // the `console.log` argument accumulator. Before #7504 those three were
    // counted as the literal's, which is what made `== 0` fail; before #7493
    // the count was zero for every program under the native default, which is
    // what made it pass for nothing. Assert the witness so neither can recur
    // silently.
    assert!(
        temp_root_slot_binds(main) > 0,
        "this fixture's `console.log` must still bind its pooled argument \
         accumulator — with no bind anywhere in the module, `zero value-slot \
         binds` is a claim about an empty set (#7504):\n{main}"
    );
}

/// The array-literal form of the same defect: `const a = [heap, n]` becomes
/// one alloca per element, and element 0 is the only reference to its value.
#[test]
fn scalar_replaced_array_element_holding_a_heap_value_is_bound() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_for(
        "scalar_array_element_root.ts",
        vec![
            let_stmt(1, "a", Expr::Array(vec![heap_value(), Expr::Number(2.0)])),
            console_log(vec![
                Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::Integer(0)),
                },
                Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::Integer(1)),
                },
            ]),
        ],
    );

    assert!(
        value_slot_binds(main_ir(&ir)) > 0,
        "the scalar-replaced array element alloca holding a heap value must be \
         bound as a precise root (#6968):\n{ir}"
    );
}

/// …and its numeric twin stays free.
#[test]
fn numeric_only_scalar_replaced_array_emits_no_rooting() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_for(
        "scalar_array_numeric.ts",
        vec![
            let_stmt(
                1,
                "a",
                Expr::Array(vec![Expr::Number(1.0), Expr::Number(2.0)]),
            ),
            console_log(vec![Expr::IndexGet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(0)),
            }]),
        ],
    );

    let main = main_ir(&ir);
    assert_eq!(
        value_slot_binds(main),
        0,
        "a scalar-replaced numeric array literal must not pay for GC rooting:\n{main}"
    );
    assert!(
        temp_root_slot_binds(main) > 0,
        "the `console.log` accumulator must still bind, so `zero value-slot \
         binds` is a claim about a module in which binds exist (#7504):\n{main}"
    );
}

/// The scalar-replaced `split()` arm: its element slots receive
/// `js_string_split_part_value` results — fresh heap strings with nothing else
/// referring to them — so they are rooted unconditionally (there is no HIR
/// expression to gate on; the value is synthesized by codegen).
///
/// Stated differentially against the same string local WITHOUT the split,
/// because the string local itself is pointer-typed and binds its own slot in
/// both compilers: only the extra binds can come from the part slots.
#[test]
fn scalar_replaced_split_parts_are_bound() {
    let _pin = NativeRootsPin::shadow();
    let source = Stmt::Let {
        id: 1,
        name: "s".to_string(),
        ty: Type::String,
        mutable: false,
        init: Some(Expr::String("a,b,c".to_string())),
    };
    let split = Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: "split".to_string(),
            byte_offset: 0,
        }),
        args: vec![Expr::String(",".to_string())],
        type_args: Vec::new(),
        byte_offset: 0,
    };
    let ir = ir_for(
        "scalar_split_parts.ts",
        vec![
            source.clone(),
            let_stmt(2, "parts", split),
            console_log(vec![
                Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(2)),
                    index: Box::new(Expr::Integer(0)),
                },
                Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(2)),
                    index: Box::new(Expr::Integer(1)),
                },
            ]),
        ],
    );
    let control = ir_for(
        "scalar_split_parts_control.ts",
        vec![source, console_log(vec![Expr::LocalGet(1)])],
    );

    assert!(
        value_slot_binds(main_ir(&ir)) > value_slot_binds(main_ir(&control)),
        "the scalar-replaced split part slots must be bound as precise roots: \
         split IR has {} value-slot binds, the split-free control has {} \
         (#6968):\n{ir}",
        value_slot_binds(main_ir(&ir)),
        value_slot_binds(main_ir(&control)),
    );
}

/// A later `o.a = <heap>` writes the same alloca and must root it too — the
/// object-literal initializer is not the only store site.
#[test]
fn later_store_into_a_scalar_replaced_field_is_bound() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_for(
        "scalar_object_field_reassign.ts",
        vec![
            let_stmt(
                1,
                "o",
                Expr::Object(vec![("a".to_string(), Expr::Number(0.0))]),
            ),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "a".to_string(),
                value: Box::new(heap_value()),
            }),
            console_log(vec![field_get(1, "a")]),
        ],
    );

    assert!(
        value_slot_binds(main_ir(&ir)) > 0,
        "a heap value assigned into a scalar-replaced field after construction \
         must be rooted as well (#6968):\n{ir}"
    );
}

// ---------------------------------------------------------------------------
// The bind is per SLOT, not per STORE (#7013).
//
// #7007 emitted `js_shadow_slot_bind` at every store into a scalar-replacement
// alloca. That call is loop-invariant apart from its root barrier: the alloca
// is entry-hoisted, so `slot_ptrs[idx]` never changes, and every reader of a
// bound slot dereferences `slot_ptrs[idx]` rather than the `stack[idx]` mirror
// the bind refreshes. In a loop it cost ~4 ns per iteration for nothing.
// ---------------------------------------------------------------------------

/// Two heap stores into the SAME scalar-replaced field must emit exactly one
/// bind — hoisted to function entry — not one per store.
///
/// Teeth: pre-hoist this IR carried one bind per store site (2), so the
/// equality fails on the old compiler. It also fails if a future change drops
/// the bind altogether (0), which would un-root the alloca and reopen #6968.
///
/// The count is per-SLOT since #7504. The whole-module version of this
/// assertion read 4 on today's compiler — one for the field, three for the
/// `console.log` accumulator's pooled temp root — so `== 1` had stopped being a
/// statement about the field at all.
#[test]
fn repeated_stores_into_one_scalar_slot_bind_once() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_for(
        "scalar_field_two_stores.ts",
        vec![
            let_stmt(
                1,
                "o",
                Expr::Object(vec![("a".to_string(), Expr::Number(0.0))]),
            ),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "a".to_string(),
                value: Box::new(heap_value()),
            }),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "a".to_string(),
                value: Box::new(heap_value()),
            }),
            console_log(vec![field_get(1, "a")]),
        ],
    );

    assert_eq!(
        value_slot_binds(main_ir(&ir)),
        1,
        "two heap stores into one scalar-replaced field must share a single \
         entry-hoisted bind — the alloca address is loop-invariant, so \
         re-binding is pure per-store cost (#7013):\n{}",
        main_ir(&ir)
    );
}

/// Every store still shades its value, so an in-flight incremental mark cannot
/// miss a pointer written into an already-scanned root.
///
/// This is the part of the bind that is genuinely per-store, and dropping it
/// while hoisting the rest would be a silent incremental-GC miscompile.
///
/// Teeth: pre-hoist the scalar-slot path emitted no
/// `js_write_barrier_root_nanbox` at all (the shading happened inside
/// `js_shadow_slot_bind`), so the old compiler produces 0 and fails.
#[test]
fn every_store_into_a_hoisted_scalar_slot_shades_its_value() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_for(
        "scalar_field_two_stores_barrier.ts",
        vec![
            let_stmt(
                1,
                "o",
                Expr::Object(vec![("a".to_string(), Expr::Number(0.0))]),
            ),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "a".to_string(),
                value: Box::new(heap_value()),
            }),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "a".to_string(),
                value: Box::new(heap_value()),
            }),
            console_log(vec![field_get(1, "a")]),
        ],
    );

    assert_eq!(
        value_slot_barriers(main_ir(&ir)),
        2,
        "each of the two heap stores must shade the value it wrote; the \
         hoisted bind only shades what the alloca held at function entry \
         (#7013). Barriers by slot: {:?}\n{}",
        perry_codegen::testing::root_slots::barriers_by_slot(main_ir(&ir)),
        main_ir(&ir)
    );

    // The barrier must be the guarded form, not an unconditional call: the
    // whole point of hoisting is that the common path (no incremental cycle in
    // flight) stays a load + compare + not-taken branch.
    assert!(
        ir.contains("@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT"),
        "the per-store shading barrier must be guarded on the incremental-mark \
         active count, otherwise the hoist just trades one unconditional call \
         for another (#7013):\n{ir}"
    );
}

/// The bind must sit in the ENTRY block, ahead of the loop that stores through
/// the slot — and after `js_shadow_frame_enter`.
///
/// This is the property the whole change exists for: a store inside a loop must
/// not re-bind per iteration. It is also where the one real ordering hazard
/// lives — `reserve_shadow_slot` can create the frame push lazily, into the very
/// region the hoisted bind is appended to, and a bind emitted before the push
/// would write a slot in the CALLER's frame.
///
/// Teeth: pre-hoist the bind was emitted at the store site, i.e. inside the
/// loop body, which is past the entry block's terminator — so the
/// `bind < first_branch` claim fails on the old compiler.
#[test]
fn bind_is_hoisted_into_the_entry_block_ahead_of_the_storing_loop() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_for(
        "scalar_field_loop_bind.ts",
        vec![
            let_stmt(9, "n", Expr::Number(3.0)),
            Stmt::While {
                condition: Expr::Compare {
                    op: perry_hir::CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(9)),
                    right: Box::new(Expr::Number(3.0)),
                },
                body: vec![
                    let_stmt(
                        1,
                        "o",
                        Expr::Object(vec![
                            ("a".to_string(), heap_value()),
                            ("b".to_string(), Expr::Number(2.0)),
                        ]),
                    ),
                    console_log(vec![field_get(1, "a"), field_get(1, "b")]),
                ],
            },
        ],
    );

    let body = main_ir(&ir);
    assert_eq!(
        value_slot_binds(body),
        1,
        "a scalar-replaced field stored once per iteration must be bound once, \
         not once per iteration (#7013):\n{body}"
    );

    // The ordering claim is about THAT bind, not about whichever bind happens
    // to come first in the function — the pooled temp roots of the in-loop
    // `console.log` bind too, inside the loop body, and a `body.find("call void
    // @js_shadow_slot_bind(")` would in general land on one of them. Name the
    // slot.
    let value_slot = perry_codegen::testing::root_slots::bound_slots(body)
        .into_iter()
        .find(|(_, (kind, _))| *kind == perry_codegen::testing::root_slots::SlotKind::Value)
        .map(|(slot, _)| slot)
        .expect("the value-slot bind was just counted");
    // #7088: the push is `js_shadow_frame_enter` (returns the state pointer).
    let push = body
        .find("call ptr @js_shadow_frame_enter(")
        .unwrap_or_else(|| panic!("no frame enter in the binding function:\n{body}"));
    let bind = body
        .find(&format!(", ptr {value_slot})"))
        .unwrap_or_else(|| panic!("no bind naming {value_slot} in:\n{body}"));
    let first_branch = body
        .find("\n  br label %")
        .unwrap_or_else(|| panic!("no entry-block terminator in:\n{body}"));

    assert!(
        push < bind,
        "the hoisted bind must follow the frame push — binding before it would \
         write a slot belonging to the caller's frame (#7013):\n{body}"
    );
    assert!(
        bind < first_branch,
        "the bind must sit in the entry block, ahead of the loop that stores \
         through the slot; emitting it at the store site is the per-iteration \
         cost this change removes (#7013):\n{body}"
    );
}

/// A scalar-replaced ARRAY element alloca must be initialized before the
/// hoisted bind makes it a live root.
///
/// Binding at entry makes the collector dereference the alloca from function
/// entry, i.e. before the element store runs and on paths where it never runs.
/// The object-literal path already stored `undefined` into its field slots at
/// entry; the array path did not, because pre-hoist nothing read those allocas
/// before their store.
///
/// Teeth: pre-hoist the array element slots got a bare `alloca` with no entry
/// store, so the `undef_stores > 0` claim fails on the old compiler.
#[test]
fn scalar_replaced_array_element_slots_are_initialized_before_the_bind() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_for(
        "scalar_array_element_init.ts",
        vec![
            let_stmt(1, "a", Expr::Array(vec![heap_value(), Expr::Number(2.0)])),
            console_log(vec![
                Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::Integer(0)),
                },
                Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::Integer(1)),
                },
            ]),
        ],
    );

    let body = main_ir(&ir);
    let value_slot = perry_codegen::testing::root_slots::bound_slots(body)
        .into_iter()
        .find(|(_, (kind, _))| *kind == perry_codegen::testing::root_slots::SlotKind::Value)
        .map(|(slot, _)| slot)
        .unwrap_or_else(|| panic!("no element slot was bound at all in:\n{body}"));
    let bind = body
        .find(&format!(", ptr {value_slot})"))
        .unwrap_or_else(|| panic!("no bind naming {value_slot} in:\n{body}"));

    // TAG_UNDEFINED as the double literal codegen emits for it.
    let undef =
        perry_codegen::nanbox::double_literal(f64::from_bits(perry_codegen::nanbox::TAG_UNDEFINED));
    let undef_store = format!("store double {undef}, ptr ");
    let stores_before_bind = body[..bind].matches(undef_store.as_str()).count();

    assert!(
        stores_before_bind > 0,
        "a scalar-replaced array element alloca is a live GC root from the \
         hoisted bind onward, so it must hold a decodable `undefined` before \
         that bind rather than uninitialized stack bytes (#7013). Looked for \
         `{undef_store}` ahead of the bind in:\n{body}"
    );
}

/// The gate still holds after hoisting: a numeric-only literal must emit
/// neither a bind nor a shading barrier.
///
/// Hoisting moves work to function entry, where it is easy to stop noticing —
/// this keeps the #6997 "a proven-numeric field costs nothing" property honest
/// for the entry region too.
#[test]
fn numeric_only_scalar_replaced_literal_emits_no_entry_rooting() {
    let _pin = NativeRootsPin::shadow();
    let ir = ir_for(
        "scalar_numeric_no_entry_rooting.ts",
        vec![
            let_stmt(
                1,
                "p",
                Expr::Object(vec![
                    ("x".to_string(), Expr::Number(1.0)),
                    ("y".to_string(), Expr::Number(2.0)),
                ]),
            ),
            console_log(vec![field_get(1, "x"), field_get(1, "y")]),
        ],
    );

    let main = main_ir(&ir);
    assert_eq!(
        value_slot_binds(main),
        0,
        "a proven-numeric literal must not acquire an entry-hoisted bind \
         (#7013):\n{main}"
    );
    assert_eq!(
        value_slot_barriers(main),
        0,
        "a proven-numeric literal must not emit a store-site shading barrier \
         (#7013). Barriers by slot: {:?}\n{main}",
        perry_codegen::testing::root_slots::barriers_by_slot(main),
    );
    assert!(
        temp_root_slot_binds(main) > 0,
        "the `console.log` accumulator must still bind, so both zeroes above \
         are claims about a module in which rooting exists (#7504):\n{main}"
    );
}
