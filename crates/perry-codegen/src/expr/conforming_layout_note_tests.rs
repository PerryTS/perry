//! Phase 4b.2 (#5094): a pointer stored into a slot the class's own pointer
//! mask declares no longer *calls* `js_gc_note_slot_layout` when the receiver
//! carries an intact side-mask descriptor.
//!
//! These are IR-census tests, and both directions matter.
//!
//! The positive one asserts the subject is LIVE — an elision predicate that
//! silently answers `false` everywhere still compiles, still prints the right
//! answer, and shows up in no other test. Only the emitted block label
//! separates "implemented" from "reached" (CLAUDE.md, "a gate must assert its
//! subject was live").
//!
//! The negatives are the safety half. The elision rests on the emitted header
//! test being paired with a slot the mask really declares a POINTER; if the
//! predicate ever widened to a raw-f64 slot, `layout_note_slot`'s downgrade arm
//! — the one that MUST fire, because a pointer in a raw-f64 slot is a
//! descriptor contradiction — would be skipped, and the collector would keep
//! reading a mask that says "not a pointer" over a live child. That is a silent
//! use-after-free, so it gets a test that fails rather than a comment.
//!
//! The fallback call is asserted PRESENT in the positive case too: the change
//! is "skip the call when the header proves it a no-op", never "elide it
//! outright" — a receiver that reached the store with no descriptor (any path
//! `class_field_store_layout_note_is_conforming`'s reasoning did not enumerate)
//! must still take the real note.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Function, Module, ModuleInitKind, Param, Stmt};

/// The block that exists only when the conforming-store elision was emitted.
const NOTE_BLOCK: &str = "class_field_set.layout_note";
const NOTE_CALL: &str = "call void @js_gc_note_slot_layout(";

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

/// `class Link { next: Link | null; v: number }` — slot 0 pointer-masked, slot
/// 1 raw-f64-masked. The two slots are what make the positive and the negative
/// test differ in exactly one thing.
fn link_class() -> Class {
    Class {
        id: 303,
        name: "Link".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![
            field(
                "next",
                Type::Union(vec![Type::Named("Link".to_string()), Type::Null]),
            ),
            field("v", Type::Number),
            // Neither pointer-bearing nor a raw-f64 candidate, so slot 2 is in
            // NEITHER mask — the case where the note is what SETS the bit.
            field("flag", Type::Boolean),
        ],
        constructor: Some(link_ctor()),
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

const A_ID: u32 = 1;
const B_ID: u32 = 2;
const CTOR_V_ID: u32 = 3;
const X_ID: u32 = 4;

/// `constructor(v: number) { this.next = null; this.v = v }` — the canonical
/// linked-structure prologue, and the shape #7686 taught
/// `ctor_prologue_param_assigned_fields` to admit. Present so the class gets a
/// keys global and an at-allocation layout declaration, exactly as `cycles.ts`
/// does; without it the test would be asking its question of a shape the real
/// compiler never produces.
fn link_ctor() -> Function {
    Function {
        id: 900,
        name: "constructor".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: CTOR_V_ID,
            name: "v".to_string(),
            ty: Type::Number,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Void,
        body: vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "next".to_string(),
                value: Box::new(Expr::Null),
            }),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "v".to_string(),
                value: Box::new(Expr::LocalGet(CTOR_V_ID)),
            }),
        ],
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

/// `function link(a: Link, b: Link) { a.<property> = <value> }`, called once
/// from module init so it is not dead.
///
/// The store lives in a FUNCTION with declared-`Link` parameters, not in module
/// init, because that is where `receiver_class_name` resolves a monomorphic
/// receiver — the same position `cycles.ts`'s `makeCycle` puts it in.
fn store_module(property: &str, value: Expr) -> Module {
    let mut m = Module::new("conforming_layout_note.ts");
    m.classes = vec![link_class()];
    let link = || Type::Named("Link".to_string());
    m.functions = vec![Function {
        id: 10,
        name: "link".to_string(),
        type_params: Vec::new(),
        params: vec![
            Param {
                id: A_ID,
                name: "a".to_string(),
                ty: link(),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
            Param {
                id: B_ID,
                name: "b".to_string(),
                ty: link(),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
        ],
        return_type: Type::Void,
        body: vec![Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::LocalGet(A_ID)),
            property: property.to_string(),
            value: Box::new(value),
        })],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    m.init = vec![
        Stmt::Let {
            id: X_ID,
            name: "x".to_string(),
            ty: link(),
            mutable: false,
            init: Some(Expr::New {
                class_name: "Link".to_string(),
                args: vec![Expr::Number(1.0)],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(10)),
            args: vec![Expr::LocalGet(X_ID), Expr::LocalGet(X_ID)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn emit(m: &Module) -> String {
    String::from_utf8(compile_module(m, ir_opts()).unwrap()).expect("LLVM IR should be UTF-8")
}

/// `a.next = b` — a pointer into the pointer-masked slot 0. The store must
/// reach the header test, and must keep the call on its cold arm.
#[test]
fn a_pointer_into_a_pointer_masked_slot_gates_the_note_on_the_header() {
    let ir = emit(&store_module("next", Expr::LocalGet(B_ID)));
    assert!(
        ir.contains(NOTE_BLOCK),
        "the conforming-store elision was not emitted for `a.next = b`; \
         `class_field_store_layout_note_is_conforming` answered false and this \
         optimization is dead:\n{ir}"
    );
    // `(GC_LAYOUT_STATE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT)` and the
    // `(SIDE_MASK | INTACT)` value it is compared against, as i16 literals.
    assert!(
        ir.contains("and i16 ") && ir.contains(", -12288") && ir.contains(", -28672"),
        "the emitted predicate is not the documented header test:\n{ir}"
    );
    assert!(
        ir.contains(NOTE_CALL),
        "the real note must survive on the cold arm — the elision is \
         'skip when the header proves it a no-op', never 'never call':\n{ir}"
    );
}

/// The same store shape into a slot the masks do **not** declare a pointer
/// (`flag: boolean` — neither pointer-bearing nor a raw-f64 candidate).
///
/// This is the load-bearing negative. There the note is not a no-op that can be
/// skipped: it is the only thing that ever sets the pointer-mask bit the
/// collector reads for that slot, so eliding it would leave a live child in an
/// object the tracer scans zero pointers of.
///
/// (The raw-f64 slot is not tested by emission: `a.v = b` with a `Link`-typed
/// value never reaches this emitter at all — `requires_raw_f64` routes it to
/// the guarded raw-f64 arm, which side-exits a non-finite value to
/// `js_put_value_set`. Asserting an absent block there would pass for a reason
/// unrelated to the elision. The mask half is covered directly, below.)
#[test]
fn a_store_into_an_undeclared_slot_keeps_an_unconditional_note() {
    let ir = emit(&store_module("flag", Expr::LocalGet(B_ID)));
    assert!(
        ir.contains(NOTE_CALL),
        "a store into a slot no mask declares must still note its layout — \
         that note is what sets the collector's pointer bit:\n{ir}"
    );
    assert!(
        !ir.contains(NOTE_BLOCK),
        "a slot outside the pointer mask must NOT take the conforming elision: \
         `layout_note_slot` there is load-bearing, not a no-op:\n{ir}"
    );
}

/// The mask predicate itself, independent of emission: it answers the pointer
/// mask, refuses a raw-f64 slot, and refuses an out-of-range index.
#[test]
fn layout_declares_pointer_slot_answers_the_pointer_mask_only() {
    use crate::typed_shape::{class_typed_layout, layout_declares_pointer_slot};
    let class = link_class();
    let mut classes = std::collections::HashMap::new();
    classes.insert("Link".to_string(), &class);
    let layout = class_typed_layout(&classes, "Link");

    assert_eq!(layout.slot_count, 3);
    assert!(
        layout_declares_pointer_slot(&layout, 0),
        "slot 0 is `Link | null` — pointer-masked"
    );
    assert!(
        !layout_declares_pointer_slot(&layout, 1),
        "slot 1 is `number` — raw-f64-masked, never pointer-masked"
    );
    assert!(
        !layout_declares_pointer_slot(&layout, 2),
        "slot 2 is `boolean` — in neither mask, so not pointer-declared"
    );
    assert!(
        !layout_declares_pointer_slot(&layout, 3),
        "slot 3 is past `slot_count`; a descriptor cannot describe it"
    );
}
