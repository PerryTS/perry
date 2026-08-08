//! repsel #7480 / #5093: the element-shape versioned loop clone must actually
//! be REACHED, and its fast clone must actually be free of the two diamonds it
//! exists to delete.
//!
//! These are IR-census tests for the same reason
//! `class_field_loop_tests.rs` is (#7287, and CLAUDE.md's "a gate must assert
//! its subject was live"): a versioned-loop matcher that declines every loop
//! still compiles, still prints the right answer, and still emits a different
//! object file than an unoptimized build. Only the emitted block labels
//! distinguish "implemented" from "reached".
//!
//! Each assertion here is paired with its negative: a loop the matcher MUST
//! decline (a store in the body, a subclass-capable element class, a
//! non-counter index) asserts the fast blocks are ABSENT, so a matcher that
//! quietly widened to admit an unsound shape fails here rather than in the gap
//! suite.

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

fn node_class(extends_name: Option<&str>) -> Class {
    Class {
        id: 202,
        name: "Node".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: extends_name.map(str::to_string),
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![
            ClassField {
                name: "v".to_string(),
                key_expr: None,
                ty: Type::Number,
                init: None,
                is_private: false,
                is_readonly: false,
                decorators: Vec::new(),
            },
            ClassField {
                name: "w".to_string(),
                key_expr: None,
                ty: Type::Number,
                init: None,
                is_private: false,
                is_readonly: false,
                decorators: Vec::new(),
            },
        ],
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

/// `keep[<index>].v`
fn elem_field(array_id: u32, index: Expr) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(array_id)),
            index: Box::new(index),
        }),
        property: "v".to_string(),
        byte_offset: 0,
    }
}

/// `sum = sum + keep[j].v` — the #7480 access shape.
fn accumulate_stmt(sum_id: u32, array_id: u32, index: Expr) -> Stmt {
    Stmt::Expr(Expr::LocalSet(
        sum_id,
        Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(sum_id)),
            right: Box::new(elem_field(array_id, index)),
        }),
    ))
}

const ARRAY_ID: u32 = 1;
const SUM_ID: u32 = 2;
const COUNTER_ID: u32 = 7;

/// `const keep: Node[] = []; let sum = 0; for (let j = 0; j < N; j++) <body>`
fn element_shape_module(body: Vec<Stmt>, extends_name: Option<&str>) -> Module {
    let mut m = Module::new("element_shape_loop.ts");
    m.classes = vec![node_class(extends_name)];
    m.init = vec![
        Stmt::Let {
            id: ARRAY_ID,
            name: "keep".to_string(),
            ty: Type::Array(Box::new(Type::Named("Node".to_string()))),
            mutable: false,
            init: Some(Expr::Array(Vec::new())),
        },
        Stmt::Let {
            id: SUM_ID,
            name: "sum".to_string(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(0.0)),
        },
        Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: COUNTER_ID,
                name: "j".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Integer(0)),
            })),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(COUNTER_ID)),
                right: Box::new(Expr::Integer(1_000_000)),
            }),
            update: Some(Expr::Update {
                id: COUNTER_ID,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body,
        },
    ];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn emit(m: &Module) -> String {
    String::from_utf8(compile_module(m, ir_opts()).unwrap()).expect("LLVM IR should be UTF-8")
}

/// The blocks that exist only when the clone was really built AND entered.
const CLONE_LABELS: [&str; 6] = [
    "element_shape.loop.preheader.brand",
    "element_shape.loop.preheader.repair",
    "element_shape.loop.preheader.query",
    "element_shape.loop.preheader.deref",
    "element_shape.loop.fast.preheader",
    "element_shape.load",
];

/// The emitted text of one named block, up to the next block label.
fn block_slice<'a>(ir: &'a str, label: &str) -> &'a str {
    let start = ir
        .find(&format!("\n{label}"))
        .unwrap_or_else(|| panic!("block `{label}` should be present in the emitted IR"));
    let body = &ir[start + 1..];
    let end = body
        .find("\n\n")
        .unwrap_or_else(|| panic!("block `{label}` should be terminated by a blank line"));
    &body[..end]
}

/// The emitted text the fast clone owns: from its cond block to the slow
/// clone's.
fn fast_clone_slice(ir: &str) -> &str {
    let start = ir
        .find("for.element_shape_fast.cond")
        .expect("fast clone cond block");
    let end = ir[start..]
        .find("for.element_shape_slow.cond")
        .map(|off| start + off)
        .unwrap_or(ir.len());
    &ir[start..end]
}

#[test]
fn element_shape_versioned_loop_fires_for_the_7480_access_shape() {
    let ir = emit(&element_shape_module(
        vec![accumulate_stmt(
            SUM_ID,
            ARRAY_ID,
            Expr::LocalGet(COUNTER_ID),
        )],
        None,
    ));
    for label in CLONE_LABELS {
        assert!(
            ir.contains(label),
            "expected the #7480 element-shape versioned loop to be lowered, but \
             `{label}` is absent from the emitted IR — the matcher in \
             stmt/element_shape_loop.rs declined"
        );
    }
    // The guard must consult the LIVE runtime invariant (#7501: a static
    // declaration gets revoked at runtime), not a compile-time assumption.
    assert!(
        ir.contains("js_array_ensure_element_shape"),
        "the preheader must call the live element-shape query"
    );
    // The slow clone survives as the cold arm — a hoisted guard with no
    // fallback is worse than no hoist.
    assert!(ir.contains("for.element_shape_slow.cond"));

    let fast = fast_clone_slice(&ir);
    // The whole point: the fast clone contains NO call at all. That is the
    // revocation argument (call-free ⇒ no funnel can revoke the invariant and
    // no allocation can move the array), so it is asserted directly rather
    // than by naming individual guard symbols.
    assert!(
        !fast.contains(" call "),
        "the fast clone must be call-free; found a call in:\n{fast}"
    );
    assert!(
        !fast.contains("@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED"),
        "the per-access inline-guard gate must be hoisted into the preheader"
    );
    assert!(
        !fast.contains("js_array_get_f64"),
        "the element-read tier must be gone from the fast clone"
    );
}

/// SABOTAGE (clone selection): a body that STORES cannot be cloned — a store
/// is the primary way to revoke the invariant mid-loop, and admitting one
/// would be a miscompile rather than a slow path.
#[test]
fn element_shape_versioned_loop_declines_a_body_that_stores() {
    let ir = emit(&element_shape_module(
        vec![
            accumulate_stmt(SUM_ID, ARRAY_ID, Expr::LocalGet(COUNTER_ID)),
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(ARRAY_ID)),
                index: Box::new(Expr::LocalGet(COUNTER_ID)),
                value: Box::new(Expr::Number(1.0)),
            }),
        ],
        None,
    ));
    for label in CLONE_LABELS {
        assert!(
            !ir.contains(label),
            "a body containing an element STORE must not be cloned, but \
             `{label}` was emitted — mid-loop revocation would make the \
             specialized body read a revoked array"
        );
    }
}

/// SUBCLASS SAFETY (#7573/#7603). An element class with a base class does not
/// get the clone: its packed slot indices are not self-describing, and an
/// `extends Array` base is the exact header-overlay hazard those issues fixed.
/// The receiver-side brand lives in the emitted IR
/// (`element_shape.loop.preheader.brand`) and is covered by the gap test.
#[test]
fn element_shape_versioned_loop_declines_a_subclass_element_type() {
    let ir = emit(&element_shape_module(
        vec![accumulate_stmt(
            SUM_ID,
            ARRAY_ID,
            Expr::LocalGet(COUNTER_ID),
        )],
        Some("Base"),
    ));
    for label in CLONE_LABELS {
        assert!(
            !ir.contains(label),
            "an element class with a base must not be cloned, but `{label}` \
             was emitted"
        );
    }
}

/// The index must be the loop counter itself. `keep[j + 1]` reads outside the
/// range the preheader's `length >= bound` check covers.
#[test]
fn element_shape_versioned_loop_declines_an_offset_index() {
    let ir = emit(&element_shape_module(
        vec![accumulate_stmt(
            SUM_ID,
            ARRAY_ID,
            Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(COUNTER_ID)),
                right: Box::new(Expr::Integer(1)),
            },
        )],
        None,
    ));
    for label in CLONE_LABELS {
        assert!(
            !ir.contains(label),
            "an offset index must not be cloned, but `{label}` was emitted"
        );
    }
}

/// SIZE DISCIPLINE (#7566's precedent): a program with no qualifying loop must
/// emit exactly zero of the clone's blocks, so the feature costs +0 bytes
/// where it does not apply.
#[test]
fn a_program_with_no_qualifying_loop_pays_nothing() {
    let mut m = Module::new("element_shape_none.ts");
    m.classes = vec![node_class(None)];
    m.init = vec![
        Stmt::Let {
            id: SUM_ID,
            name: "sum".to_string(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(0.0)),
        },
        Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: COUNTER_ID,
                name: "j".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Integer(0)),
            })),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(COUNTER_ID)),
                right: Box::new(Expr::Integer(1_000_000)),
            }),
            update: Some(Expr::Update {
                id: COUNTER_ID,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body: vec![Stmt::Expr(Expr::LocalSet(
                SUM_ID,
                Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(SUM_ID)),
                    right: Box::new(Expr::Integer(1)),
                }),
            ))],
        },
    ];
    m.init_kind = ModuleInitKind::Eager;
    let ir = emit(&m);
    for label in CLONE_LABELS {
        assert!(
            !ir.contains(label),
            "a module with no qualifying loop must not emit `{label}`"
        );
    }
    // The runtime declaration is emitted for every module (an unused
    // `declare` costs nothing in the object); what must be absent is a CALL.
    assert!(
        !ir.contains("call i32 @js_array_ensure_element_shape"),
        "a module with no qualifying loop must not call the guard"
    );
}

// ---------------------------------------------------------------------------
// #7480: growth-forwarding repair.
//
// `js_array_grow` allocates the larger array elsewhere and leaves a forwarding
// stub behind whose first payload word (`length`‖`capacity`) is OVERWRITTEN
// with the new head. Every runtime entry point resolves the chain, so the
// guard call answers about the LIVE array — but the emitted code reads
// `length` and the elements base off the raw pointer, and on a stub both are
// wrong in the worst possible combination: `length` reads the low half of a
// heap pointer (a huge number, so the `length >= bound` test PASSES), while
// the base still addresses the pre-growth buffer. The clone then reads correct
// elements up to the old capacity and runs off the end of the block after it —
// masked, dereferenced at `-8`, SIGBUS. With `MIN_ARRAY_CAPACITY == 16` that
// is exactly the "right answer at 16 elements, bus error at 17" shape #7480
// reproduced.
//
// The repair is repsel 4a.2's (#6904) self-heal: follow the chain once and
// write the live head back to the binding, BEFORE the guard call — the refresh
// can itself allocate (a lazy array materializes inside `clean_arr_ptr`), so
// doing it afterwards would reintroduce the very "base derived across an
// allocating call" hazard that step (4) exists to avoid.
// ---------------------------------------------------------------------------

#[test]
fn preheader_repairs_the_array_head_before_the_guard_call() {
    let ir = emit(&element_shape_module(
        vec![accumulate_stmt(
            SUM_ID,
            ARRAY_ID,
            Expr::LocalGet(COUNTER_ID),
        )],
        None,
    ));
    let repair = block_slice(&ir, "element_shape.loop.preheader.repair");
    assert!(
        repair.contains("call double @js_array_refresh_local_head"),
        "the repair block must follow the growth-forwarding chain; emitted:\n{repair}"
    );

    // Ordering is the whole fix. A refresh emitted AFTER the guard call would
    // leave the elements base derived from the stub.
    let refresh_at = ir
        .find("call double @js_array_refresh_local_head")
        .expect("growth-forwarding refresh call");
    let query_at = ir
        .find("call i32 @js_array_ensure_element_shape")
        .expect("element-shape guard call");
    assert!(
        refresh_at < query_at,
        "the growth-forwarding refresh must precede the element-shape query"
    );
}

#[test]
fn the_repaired_head_is_written_back_to_the_binding() {
    let ir = emit(&element_shape_module(
        vec![accumulate_stmt(
            SUM_ID,
            ARRAY_ID,
            Expr::LocalGet(COUNTER_ID),
        )],
        None,
    ));
    let repair = block_slice(&ir, "element_shape.loop.preheader.repair");

    // WITHOUT a write-back the repair would be inert: the query and deref
    // blocks both RE-READ the binding, so they would read the stub straight
    // back out — which is precisely how the bug shipped. So the assertion is
    // not "a store exists" but "the value stored is the refresh's result".
    let refreshed = repair
        .lines()
        .find_map(|l| l.trim().split_once(" = call double @js_array_refresh_local_head"))
        .map(|(reg, _)| reg.to_string())
        .expect("the refresh call should bind a register");
    assert!(
        repair.lines().any(|l| l.trim().starts_with("store ")),
        "the repair block must write the live head back; emitted:\n{repair}"
    );
    // A root-slot store is rewritten by `function/precise_roots.rs` into
    // `bitcast double <reg> to i64` + `inttoptr` + `store ptr addrspace(1)`,
    // so the bitcast naming the refresh's result IS the write-back.
    assert!(
        repair.lines().any(|l| {
            let l = l.trim();
            l.starts_with("%rs4gc.b") && l.contains(&format!("bitcast double {refreshed} to i64"))
        }),
        "the value stored back must be the refreshed head {refreshed}; emitted:\n{repair}"
    );
}

#[test]
fn the_repair_does_not_put_a_call_inside_the_fast_clone() {
    let ir = emit(&element_shape_module(
        vec![accumulate_stmt(
            SUM_ID,
            ARRAY_ID,
            Expr::LocalGet(COUNTER_ID),
        )],
        None,
    ));
    // The repair adds a call to the PREHEADER, which is fine. One inside the
    // clone would void the revocation argument — and, because the lowering
    // then branches unconditionally to the slow clone, would silently delete
    // the optimization instead of failing.
    let fast = fast_clone_slice(&ir);
    assert!(
        !fast.contains("call "),
        "the fast clone must stay call-free after the #7480 repair; emitted:\n{fast}"
    );
    assert!(
        ir.contains("element_shape.loop.fast.preheader"),
        "the clone must still be reached after the #7480 repair"
    );
}
