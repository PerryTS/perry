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

/// A closed-shape object literal's synthesized class, exactly as
/// `perry-hir`'s `mint_anon_shape_class` builds it: content-addressed name, no
/// base, no computed members, fields in source order with `init: None`.
fn anon_shape_class(id: u32, name: &str, fields: &[(&str, Type)]) -> Class {
    let mut class = node_class(None);
    class.id = id;
    class.name = name.to_string();
    class.fields = fields
        .iter()
        .map(|(field, ty)| ClassField {
            name: (*field).to_string(),
            key_expr: None,
            ty: ty.clone(),
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        })
        .collect();
    class
}

/// The declared element type of #7480's own kernel: `{ v: number; w: number }`.
fn object_element_type(fields: &[(&str, Type)], optional: bool) -> Type {
    let mut properties = std::collections::HashMap::new();
    let mut property_order = Vec::new();
    for (name, ty) in fields {
        property_order.push((*name).to_string());
        properties.insert(
            (*name).to_string(),
            perry_hir::types::PropertyInfo {
                ty: ty.clone(),
                optional,
                readonly: false,
            },
        );
    }
    Type::Object(perry_hir::types::ObjectType {
        name: None,
        properties,
        property_order: Some(property_order),
        index_signature: None,
    })
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

/// `const keep: <elem>[] = []; let sum = 0; for (let j = 0; j < N; j++) sum +=
/// keep[j].v` with an explicit set of module classes — the object-literal
/// twin of [`element_shape_module`] (#7480 step 3).
fn object_element_module(elem: Type, classes: Vec<Class>) -> Module {
    let mut m = element_shape_module(
        vec![accumulate_stmt(
            SUM_ID,
            ARRAY_ID,
            Expr::LocalGet(COUNTER_ID),
        )],
        None,
    );
    m.classes = classes;
    // A silent skip here would leave the array typed `Node[]` and every
    // object-literal test below would quietly be re-testing the named-class
    // path — passing for the wrong reason. Assert the shape instead.
    let Some(Stmt::Let { ty, .. }) = m.init.first_mut() else {
        panic!("element_shape_module's first init statement should be the array `Let`");
    };
    *ty = Type::Array(Box::new(elem));
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

/// Byte offset of the DEFINITION of block `label` — a line that begins at
/// column 0 with `<label>` and ends in `:`. A bare `ir.find(label)` finds the
/// `br label %<label>` reference instead, which is a different place entirely.
fn block_def_offset(ir: &str, label: &str) -> Option<usize> {
    let mut offset = 0usize;
    for line in ir.split_inclusive('\n') {
        if line.starts_with(label) && line.trim_end().ends_with(':') {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// The emitted text the fast clone owns: from the DEFINITION of its cond block
/// to the definition of the slow clone's. Covers `for.element_shape_fast.*`
/// and the `element_shape.load` block the field read branches into.
///
/// #7480 step 3 — ANTI-VACUITY. This used to slice from the first *substring*
/// occurrence of `for.element_shape_fast.cond`, which is the
/// `br label %for.element_shape_fast.cond.N` terminator of
/// `element_shape.loop.fast.preheader`, three lines above the slow
/// preheader's own branch. Every assertion made against the result is a
/// NEGATIVE (`!fast.contains(" call ")`, `!fast.contains("js_array_get_f64")`,
/// …), so that four-line stub satisfied all of them: the IR census that exists
/// to prove the clone is really call-free had never been able to fail. The
/// liveness assertion below is what makes it a gate — CLAUDE.md's "a gate must
/// assert its subject was live".
fn fast_clone_slice(ir: &str) -> &str {
    let start = block_def_offset(ir, "for.element_shape_fast.cond")
        .expect("the fast clone's cond block should be DEFINED in the emitted IR");
    let end = block_def_offset(ir, "for.element_shape_slow.cond").unwrap_or(ir.len());
    assert!(end > start, "the fast clone must precede the slow clone");
    let slice = &ir[start..end];
    assert!(
        slice.contains("for.element_shape_fast.body") && slice.contains("element_shape.load"),
        "the fast-clone slice must contain the cloned BODY and its element \
         load, otherwise every negative assertion against it is vacuous; \
         sliced:\n{slice}"
    );
    slice
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
        .find_map(|l| {
            l.trim()
                .split_once(" = call double @js_array_refresh_local_head")
        })
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

// ---------------------------------------------------------------------------
// #7480 step 3: OBJECT-LITERAL element types.
//
// `keep: {v: number, w: number}[]` is #7480's own kernel and the whole
// measured gap — 414 ms against node's 12 on 200k x 50, where the named-class
// arm the clone already covered is 13 ms. The literals allocate an
// `__AnonShape_<hash>`, so a class id genuinely exists; the matcher resolves
// it by matching the declared property order against the module's anon shapes.
//
// The two halves are NOT separable, which is why the assertions below are in
// one test: with no resolvable class the accumulator also loses its numeric
// proof, `sum += keep[j].v` lowers through `js_dynamic_string_or_number_add`,
// and THAT CALL fails the clone's call-free admission — so a fix that resolved
// the class without also restoring the numeric proof would emit a block of
// dead fast-clone IR and change nothing.
// ---------------------------------------------------------------------------

const ANON_SHAPE_VW: &str = "__AnonShape_0000000000000abc";

#[test]
fn element_shape_versioned_loop_resolves_an_object_literal_element_type() {
    let ir = emit(&object_element_module(
        object_element_type(&[("v", Type::Number), ("w", Type::Number)], false),
        vec![anon_shape_class(
            311,
            ANON_SHAPE_VW,
            &[("v", Type::Number), ("w", Type::Number)],
        )],
    ));
    for label in CLONE_LABELS {
        assert!(
            ir.contains(label),
            "#7480's own kernel (an object-literal element type) must reach the \
             clone, but `{label}` is absent — the matcher declined"
        );
    }
    // The guard must be pinned to the ANON SHAPE's keys global, not to some
    // other class that happened to be in scope.
    let deref = block_slice(&ir, "element_shape.loop.preheader.deref");
    assert!(
        deref.contains("AnonShape"),
        "the preheader must load the anon shape's keys global; emitted:\n{deref}"
    );

    let fast = fast_clone_slice(&ir);
    assert!(
        !fast.contains(" call "),
        "the fast clone must be call-free; found a call in:\n{fast}"
    );
    // The second half, and the reason the first half alone would be inert: the
    // accumulate is a real `fadd`, not the BigInt-aware dynamic add helper.
    assert!(
        !fast.contains("js_dynamic_string_or_number_add"),
        "the accumulator must regain its numeric proof inside the clone; \
         emitted:\n{fast}"
    );
    assert!(
        fast.contains("fadd"),
        "the accumulate must lower to an fadd inside the clone; emitted:\n{fast}"
    );
}

/// SABOTAGE (resolution): two anon shapes with the same field NAMES and
/// incompatible field types are ambiguous. `ctx.classes` is a `HashMap`, so
/// "pick whichever came first" would make the emitted code depend on iteration
/// order; the resolver declines instead.
#[test]
fn element_shape_versioned_loop_declines_an_ambiguous_object_shape() {
    let ir = emit(&object_element_module(
        // `any` rules nothing out, so BOTH shapes below stay compatible and
        // the tie cannot be broken.
        object_element_type(&[("v", Type::Any), ("w", Type::Any)], false),
        vec![
            anon_shape_class(
                311,
                ANON_SHAPE_VW,
                &[("v", Type::Number), ("w", Type::Number)],
            ),
            anon_shape_class(
                312,
                "__AnonShape_0000000000000def",
                &[("v", Type::String), ("w", Type::String)],
            ),
        ],
    ));
    for label in CLONE_LABELS {
        assert!(
            !ir.contains(label),
            "an ambiguous object shape must not be cloned, but `{label}` was \
             emitted — the pick would depend on HashMap iteration order"
        );
    }
}

/// A tie that field types CAN break is resolved rather than declined: only one
/// of the two same-named shapes is numeric, and the declaration says `number`.
#[test]
fn element_shape_versioned_loop_breaks_a_tie_on_field_types() {
    let ir = emit(&object_element_module(
        object_element_type(&[("v", Type::Number), ("w", Type::Number)], false),
        vec![
            anon_shape_class(
                311,
                ANON_SHAPE_VW,
                &[("v", Type::Number), ("w", Type::Number)],
            ),
            anon_shape_class(
                312,
                "__AnonShape_0000000000000def",
                &[("v", Type::String), ("w", Type::String)],
            ),
        ],
    ));
    for label in CLONE_LABELS {
        assert!(
            ir.contains(label),
            "a type-disambiguated object shape should still be cloned, but \
             `{label}` is absent"
        );
    }
}

/// SABOTAGE (closedness): an OPTIONAL property means the runtime object may
/// not have that slot at all, so the declared order names no layout.
#[test]
fn element_shape_versioned_loop_declines_an_optional_property() {
    let ir = emit(&object_element_module(
        object_element_type(&[("v", Type::Number), ("w", Type::Number)], true),
        vec![anon_shape_class(
            311,
            ANON_SHAPE_VW,
            &[("v", Type::Number), ("w", Type::Number)],
        )],
    ));
    for label in CLONE_LABELS {
        assert!(
            !ir.contains(label),
            "an optional property must not be cloned, but `{label}` was emitted"
        );
    }
}

/// SABOTAGE (no allocation site): an object type whose shape no literal in the
/// module allocates has no class id to guard on. The declared type is a hint,
/// and a hint with no referent must decline rather than invent one.
#[test]
fn element_shape_versioned_loop_declines_an_unallocated_object_shape() {
    let ir = emit(&object_element_module(
        object_element_type(&[("v", Type::Number), ("w", Type::Number)], false),
        vec![anon_shape_class(
            311,
            ANON_SHAPE_VW,
            &[("x", Type::Number), ("y", Type::Number)],
        )],
    ));
    for label in CLONE_LABELS {
        assert!(
            !ir.contains(label),
            "an object shape with no matching anon class must not be cloned, \
             but `{label}` was emitted"
        );
    }
}

/// SABOTAGE (field order): the anon shape's slot layout is SOURCE ORDER, so a
/// same-named-but-reordered shape is a different layout. Matching it would
/// read `w` where the loop asked for `v`.
#[test]
fn element_shape_versioned_loop_declines_a_reordered_object_shape() {
    let ir = emit(&object_element_module(
        object_element_type(&[("v", Type::Number), ("w", Type::Number)], false),
        vec![anon_shape_class(
            311,
            ANON_SHAPE_VW,
            &[("w", Type::Number), ("v", Type::Number)],
        )],
    ));
    for label in CLONE_LABELS {
        assert!(
            !ir.contains(label),
            "a reordered object shape must not be cloned, but `{label}` was \
             emitted — its packed slot indices describe a different layout"
        );
    }
}

/// The object-literal resolution must not leak out of the matcher. A read of
/// the same array OUTSIDE any element-shape loop still has no resolvable
/// receiver class, which is the #6377 containment `receiver_class_name` was
/// deliberately not widened for.
#[test]
fn object_literal_element_resolution_does_not_escape_the_clone() {
    let mut m = object_element_module(
        object_element_type(&[("v", Type::Number), ("w", Type::Number)], false),
        vec![anon_shape_class(
            311,
            ANON_SHAPE_VW,
            &[("v", Type::Number), ("w", Type::Number)],
        )],
    );
    // A straight-line read after the loop, at a constant index.
    m.init.push(Stmt::Expr(Expr::LocalSet(
        SUM_ID,
        Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(SUM_ID)),
            right: Box::new(elem_field(ARRAY_ID, Expr::Integer(0))),
        }),
    )));
    let ir = emit(&m);
    assert!(
        ir.contains("element_shape.loop.fast.preheader"),
        "the loop must still be cloned"
    );
    // Outside the clone the read keeps the generic by-name lowering. If the
    // resolver had been wired into `receiver_class_name` instead, this read
    // would have become a class-field diamond too — a change this PR did not
    // measure.
    //
    // Sliced from the merge block's DEFINITION, not from the first mention of
    // its name: the earliest occurrence is a `br label %element_shape.loop.merge`
    // inside the fast clone, and slicing from there would let the CLONE's own
    // instructions satisfy an assertion about what follows it.
    let after = &ir[block_def_offset(&ir, "element_shape.loop.merge")
        .expect("the merge block should be DEFINED in the emitted IR")..];
    assert!(
        after.contains("js_object_get_field_by_name_f64")
            || after.contains("js_object_get_field_ic_miss"),
        "the post-loop read must stay on the by-name path; emitted:\n{after}"
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
