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
        namespace_member_nested: Vec::new(),
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

/// [`emit`] with module-level `type X = …` aliases in scope.
fn emit_with_aliases(m: &Module, aliases: &[(&str, Type)]) -> String {
    let mut opts = ir_opts();
    opts.type_aliases = aliases
        .iter()
        .map(|(name, ty)| ((*name).to_string(), ty.clone()))
        .collect();
    String::from_utf8(compile_module(m, opts).unwrap()).expect("LLVM IR should be UTF-8")
}

/// `<receiver>.length`
fn length_of(receiver_id: u32) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(receiver_id)),
        property: "length".to_string(),
        byte_offset: 0,
    }
}

/// Replace the module's loop condition with `j < <bound>`.
///
/// Mutates in place and panics if the statement is not where
/// [`element_shape_module`] puts it, for the same reason
/// [`object_element_module`] asserts its own shape: a silent skip would leave
/// the constant bound in place and every `.length` assertion below would be
/// re-testing the constant-bound path.
fn with_bound(m: &mut Module, bound: Expr) {
    let Some(Stmt::For { condition, .. }) = m.init.get_mut(2) else {
        panic!("element_shape_module's third init statement should be the `For`");
    };
    *condition = Some(Expr::Compare {
        op: CompareOp::Lt,
        left: Box::new(Expr::LocalGet(COUNTER_ID)),
        right: Box::new(bound),
    });
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

/// The clone must be **entered**, not merely emitted.
///
/// `lower_element_shape_versioned_for` builds the fast clone first and proves
/// it call-free second. When the proof fails it terminates the deref block with
/// an *unconditional* branch to the slow clone and leaves the fast blocks as
/// unreachable code. Every [`CLONE_LABELS`] assertion still passes in that
/// state — all the labels are present — so the census alone cannot tell
/// "optimized" from "optimization silently deleted".
///
/// That gap is not hypothetical. #7690 restored moving-loop back-edge polls to
/// ON by default, which put a `js_gc_loop_safepoint()` call in the clone's
/// element-load block; the whole clone became dead code, the benchmark it was
/// written for did not move, and not one test failed. The module docs of
/// `stmt/element_shape_loop.rs` named this failure mode in advance ("a silent
/// loss of the optimization … with no test failing"). This is the assertion
/// that makes it loud.
fn assert_fast_clone_is_entered(ir: &str) {
    let deref = block_slice(ir, "element_shape.loop.preheader.deref");
    assert!(
        deref.contains("label %element_shape.loop.fast.preheader"),
        "the guard must branch INTO the fast clone. The deref block ends in an \
         unconditional branch to the slow clone, which means the call-free \
         proof failed and the clone is dead code:\n{deref}"
    );
}

/// The emitted text the fast clone owns: exactly the blocks named
/// `for.element_shape_fast.*` and the `element_shape.load` blocks its field
/// reads branch into.
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
///
/// #7480 step 4 — the OPPOSITE error, and the reason this now selects blocks by
/// name instead of slicing a span. "Everything between the fast cond and the
/// slow cond" is only the fast clone while nothing else is emitted in between.
/// An `arr.length` bound makes the slow clone hoist its own length read, and
/// those `plen.*` blocks land in the gap — so a call belonging to the SLOW
/// clone was attributed to the fast one and the census failed on a fast clone
/// that was, and still is, bare. A span that depends on what a neighbour emits
/// can report either way; ownership is a property of the block, so ask the
/// block.
fn fast_clone_slice(ir: &str) -> String {
    let mut owned = String::new();
    let mut in_fast_block = false;
    for line in ir.split_inclusive('\n') {
        let trimmed = line.trim_end();
        // A block DEFINITION starts at column 0 and ends in `:`; anything else
        // belongs to whichever block was last opened.
        if !line.starts_with(char::is_whitespace) && trimmed.ends_with(':') {
            in_fast_block = trimmed.starts_with("for.element_shape_fast.")
                || trimmed.starts_with("element_shape.load");
        }
        if in_fast_block {
            owned.push_str(line);
        }
    }
    assert!(
        owned.contains("for.element_shape_fast.body") && owned.contains("element_shape.load"),
        "the fast-clone slice must contain the cloned BODY and its element \
         load, otherwise every negative assertion against it is vacuous; \
         sliced:\n{owned}"
    );
    owned
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

    assert_fast_clone_is_entered(&ir);
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

    assert_fast_clone_is_entered(&ir);
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

// ---------------------------------------------------------------------------
// #7480 step 4: the two reasons `churn_read.ts` never reached the clone.
//
// #7612 landed the clone and #7669 taught it object-literal element types, and
// the benchmark the whole route was chosen for did not move by one millisecond
// across both. Not because the lowering was wrong — because the MATCHER never
// got to it. `for (let j = 0; j < keep.length; j++)` fails the bound match
// (`keep.length` is a `PropertyGet`), and `type Node = {v: number}` makes
// `element_class_name` answer "Node", a name no class owns, which returned
// early and skipped the anon-shape resolver #7669 had just added.
//
// Each is independently fatal, which is why they are tested independently and
// then together in the shape the benchmark actually has.
// ---------------------------------------------------------------------------

const ANON_SHAPE_ALIASED: &str = "__AnonShape_0000000000000def";

#[test]
fn element_shape_versioned_loop_admits_an_array_length_bound() {
    let mut m = object_element_module(
        object_element_type(&[("v", Type::Number), ("w", Type::Number)], false),
        vec![anon_shape_class(
            311,
            ANON_SHAPE_VW,
            &[("v", Type::Number), ("w", Type::Number)],
        )],
    );
    with_bound(&mut m, length_of(ARRAY_ID));
    let ir = emit(&m);
    for label in CLONE_LABELS {
        assert!(
            ir.contains(label),
            "`for (j = 0; j < arr.length; j++)` is the idiom every #7480 kernel \
             is written in and must reach the clone, but `{label}` is absent"
        );
    }

    assert_fast_clone_is_entered(&ir);
    let fast = fast_clone_slice(&ir);
    assert!(
        !fast.contains(" call "),
        "the fast clone must stay call-free with a `.length` bound; found a \
         call in:\n{fast}"
    );
    // The trip count is the word the guard already loaded. If the bound were
    // re-derived per iteration the clone would still be correct and would win
    // far less, and nothing else here would notice.
    assert!(
        !fast.contains("js_array_length"),
        "the `.length` bound must be materialized once in the preheader, not \
         re-read inside the clone; emitted:\n{fast}"
    );
    assert!(
        fast.contains("fadd"),
        "the accumulate must lower to an fadd inside the clone; emitted:\n{fast}"
    );
}

/// SABOTAGE (bound provenance): the guard's `length` load answers for the
/// array the guard branded. Reading `keep[j]` while trip-counting on a
/// DIFFERENT array's length is an out-of-range read, not a slow clone.
#[test]
fn element_shape_versioned_loop_declines_a_foreign_arrays_length_bound() {
    const OTHER_ARRAY_ID: u32 = 41;
    let mut m = object_element_module(
        object_element_type(&[("v", Type::Number), ("w", Type::Number)], false),
        vec![anon_shape_class(
            311,
            ANON_SHAPE_VW,
            &[("v", Type::Number), ("w", Type::Number)],
        )],
    );
    m.init.insert(
        0,
        Stmt::Let {
            id: OTHER_ARRAY_ID,
            name: "other".to_string(),
            ty: Type::Array(Box::new(Type::Number)),
            mutable: false,
            init: Some(Expr::Array(Vec::new())),
        },
    );
    // `element_shape_module` puts the `For` at index 2; the insert above
    // shifted it to 3, so rebuild the condition by hand rather than through
    // `with_bound`'s fixed index.
    let Some(Stmt::For { condition, .. }) = m.init.get_mut(3) else {
        panic!("the `For` should have shifted to index 3");
    };
    *condition = Some(Expr::Compare {
        op: CompareOp::Lt,
        left: Box::new(Expr::LocalGet(COUNTER_ID)),
        right: Box::new(length_of(OTHER_ARRAY_ID)),
    });

    let ir = emit(&m);
    for label in CLONE_LABELS {
        assert!(
            !ir.contains(label),
            "trip-counting on `other.length` while reading `keep[j]` must not \
             be cloned, but `{label}` was emitted — the preheader proves \
             nothing about how the two lengths relate"
        );
    }
}

#[test]
fn element_shape_versioned_loop_resolves_an_aliased_object_element_type() {
    let mut m = object_element_module(
        Type::Named("Node".to_string()),
        vec![anon_shape_class(
            312,
            ANON_SHAPE_ALIASED,
            &[("v", Type::Number), ("w", Type::Number)],
        )],
    );
    // `type Node = { v: number; w: number }` — the alias is the ONLY thing
    // standing between this module and the previous test's.
    with_bound(&mut m, Expr::Integer(1_000_000));
    let ir = emit_with_aliases(
        &m,
        &[(
            "Node",
            object_element_type(&[("v", Type::Number), ("w", Type::Number)], false),
        )],
    );
    for label in CLONE_LABELS {
        assert!(
            ir.contains(label),
            "an alias-typed element (`type Node = {{…}}`) must resolve to the \
             anon shape its literals allocate, but `{label}` is absent"
        );
    }
    let deref = block_slice(&ir, "element_shape.loop.preheader.deref");
    assert!(
        deref.contains("AnonShape"),
        "the guard must pin the anon shape the alias expands to; emitted:\n{deref}"
    );
    // ANTI-VACUITY. `CLONE_LABELS` above is satisfied by a preheader that ends
    // in an unconditional branch to the SLOW clone -- which is exactly what a
    // failed call-free proof emits, and exactly the silent-deletion shape the
    // rest of this file exists to catch. Asserting the labels alone would let
    // alias resolution "work" while the clone it resolved was dead code.
    //
    // The sibling `.length` test carries these two assertions; this one, the
    // only test covering alias resolution IN ISOLATION, did not. So a
    // regression confined to the alias-without-a-`.length`-bound path -- the
    // one shape no other test emits -- would have gone unseen.
    assert_fast_clone_is_entered(&ir);
    let fast = fast_clone_slice(&ir);
    assert!(
        !fast.contains(" call "),
        "an alias-resolved fast clone must stay call-free -- a call is a GC \
         safepoint, and the element-shape guard is established BEFORE it; \
         emitted:\n{fast}"
    );
}

/// SABOTAGE (alias resolution): an alias is not a licence to guess. With no
/// `type Node = …` in scope, `Node[]` names a class that does not exist and
/// the matcher must decline rather than fall through to whatever single anon
/// shape happens to be in the module.
#[test]
fn element_shape_versioned_loop_declines_an_unresolvable_named_element_type() {
    let ir = emit(&object_element_module(
        Type::Named("Node".to_string()),
        vec![anon_shape_class(
            312,
            ANON_SHAPE_ALIASED,
            &[("v", Type::Number), ("w", Type::Number)],
        )],
    ));
    for label in CLONE_LABELS {
        assert!(
            !ir.contains(label),
            "`Node[]` with no class and no alias names no layout, but \
             `{label}` was emitted — the resolver guessed"
        );
    }
}

/// `churn_read.ts` itself: an aliased object element type AND an `arr.length`
/// bound, the combination the benchmark has had since it was written. Both
/// halves above pass individually; this is the one that was red for two
/// shipped PRs.
#[test]
fn element_shape_versioned_loop_fires_for_the_churn_read_shape() {
    let mut m = object_element_module(
        Type::Named("Node".to_string()),
        vec![anon_shape_class(
            312,
            ANON_SHAPE_ALIASED,
            &[("v", Type::Number), ("w", Type::Number)],
        )],
    );
    with_bound(&mut m, length_of(ARRAY_ID));
    let ir = emit_with_aliases(
        &m,
        &[(
            "Node",
            object_element_type(&[("v", Type::Number), ("w", Type::Number)], false),
        )],
    );
    for label in CLONE_LABELS {
        assert!(
            ir.contains(label),
            "`churn_read.ts`'s exact shape must reach the clone, but `{label}` \
             is absent"
        );
    }
    assert_fast_clone_is_entered(&ir);
    let fast = fast_clone_slice(&ir);
    assert!(
        !fast.contains(" call "),
        "the fast clone must be call-free; found a call in:\n{fast}"
    );
    assert!(
        !fast.contains("js_dynamic_string_or_number_add"),
        "the accumulate must regain its numeric proof; emitted:\n{fast}"
    );
    assert!(
        fast.contains("fadd"),
        "the accumulate must lower to an fadd; emitted:\n{fast}"
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
    assert_fast_clone_is_entered(&ir);
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

// ── #7771: the element-binding body form ────────────────────────────────────
//
// `for (let j = 0; j < N; j++) { const r = keep[j]; sum = sum + r.v; }` — the
// shape real read loops are written in. The binding is VIRTUAL inside the
// fast clone: its `Let` emits nothing and every `r.field` lowers through the
// fact, so the element fetch is a bare load under the preheader guard instead
// of the generic index-get's runtime-call diamond.

const BINDING_ID: u32 = 9;

/// `const r = keep[j];` (or `let r = ...` when `mutable`).
fn binding_stmt(mutable: bool) -> Stmt {
    Stmt::Let {
        id: BINDING_ID,
        name: "r".to_string(),
        ty: Type::Named("Node".to_string()),
        mutable,
        init: Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(ARRAY_ID)),
            index: Box::new(Expr::LocalGet(COUNTER_ID)),
        }),
    }
}

/// `sum = sum + <value>`
fn binding_accumulate(value: Expr) -> Stmt {
    Stmt::Expr(Expr::LocalSet(
        SUM_ID,
        Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(SUM_ID)),
            right: Box::new(value),
        }),
    ))
}

/// `r.<prop>`
fn binding_field(prop: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(BINDING_ID)),
        property: prop.to_string(),
        byte_offset: 0,
    }
}

#[test]
fn element_shape_versioned_loop_fires_for_the_7771_element_binding_shape() {
    let ir = emit(&element_shape_module(
        vec![binding_stmt(false), binding_accumulate(binding_field("v"))],
        None,
    ));
    for label in CLONE_LABELS {
        assert!(
            ir.contains(label),
            "expected the element-binding form to be admitted, but `{label}` is \
             absent — the matcher in stmt/element_shape_loop.rs declined the \
             two-statement body"
        );
    }
    // Entered, not merely emitted. This is ALSO the assertion that proves the
    // binding's `Let` really was skipped: had it lowered generically, its
    // index-get diamond would put `js_array_get_f64` inside the clone, the
    // call-free scan would fail, and the guard would branch unconditionally
    // to the slow clone (#7690's deletion mode).
    assert_fast_clone_is_entered(&ir);
    let fast = fast_clone_slice(&ir);
    assert!(
        !fast.contains(" call "),
        "the fast clone must be call-free; found a call in:\n{fast}"
    );
    assert!(
        !fast.contains("js_array_get_f64"),
        "the element fetch must be a bare load in the fast clone"
    );
    // The slow clone is the unchanged generic body: it must still bind `r`
    // through the generic index-get, whose runtime call handles every shape
    // the guard declines (holes, forwarding stubs, descriptors, subclasses).
    assert!(
        ir.contains("js_array_get_f64"),
        "the slow clone must keep the generic element fetch"
    );
}

#[test]
fn element_binding_reads_every_tracked_field_through_the_fact() {
    let ir = emit(&element_shape_module(
        vec![
            binding_stmt(false),
            binding_accumulate(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(binding_field("v")),
                right: Box::new(binding_field("w")),
            }),
        ],
        None,
    ));
    assert_fast_clone_is_entered(&ir);
    let fast = fast_clone_slice(&ir);
    assert!(
        !fast.contains(" call "),
        "the two-field fast clone must be call-free; found a call in:\n{fast}"
    );
    // One `element_shape.load` chain per tracked field read.
    let load_defs = ir
        .lines()
        .filter(|l| l.starts_with("element_shape.load") && l.trim_end().ends_with(':'))
        .count();
    assert!(
        load_defs >= 2,
        "both `r.v` and `r.w` must lower through the fact's bare load; found \
         {load_defs} element_shape.load block(s)"
    );
}

/// SABOTAGE (scoping): a `let`/`var` binding is not admitted. `var` is
/// function-scoped and observable after the loop, where the skipped `Let`
/// would leave the slot holding its pre-loop value; the matcher keys on
/// `mutable: false`, so this must decline rather than emit a clone.
#[test]
fn element_binding_declines_a_mutable_binding() {
    let ir = emit(&element_shape_module(
        vec![binding_stmt(true), binding_accumulate(binding_field("v"))],
        None,
    ));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "a mutable element binding must decline the clone"
    );
}

/// SABOTAGE (escape): the binding used as a bare VALUE hands out a reference
/// the clone's skipped `Let` never bound. The walk must exclude it
/// EXPLICITLY — not via the numeric-type test, which a lying annotation
/// could satisfy.
#[test]
fn element_binding_declines_a_bare_value_use() {
    let ir = emit(&element_shape_module(
        vec![
            binding_stmt(false),
            binding_accumulate(Expr::LocalGet(BINDING_ID)),
        ],
        None,
    ));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "a bare use of the element binding must decline the clone"
    );
}

/// SABOTAGE (field admission): a property the element class does not declare
/// as a raw-f64 slot has no packed index to load; the per-prop admission
/// must decline the whole loop for the binding form exactly as it does for
/// the `arr[j].field` form.
#[test]
fn element_binding_declines_an_untracked_field() {
    let ir = emit(&element_shape_module(
        vec![
            binding_stmt(false),
            binding_accumulate(binding_field("missing")),
        ],
        None,
    ));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "a field the class does not declare must decline the clone"
    );
}

/// SABOTAGE (one array per loop): the binding pins the array it was fetched
/// from; a body that ALSO reads a different array's element must decline via
/// the walk's one-array rule.
#[test]
fn element_binding_declines_a_second_array() {
    const OTHER_ID: u32 = 11;
    let mut m = element_shape_module(
        vec![
            Stmt::Let {
                id: BINDING_ID,
                name: "r".to_string(),
                ty: Type::Named("Node".to_string()),
                mutable: false,
                init: Some(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(OTHER_ID)),
                    index: Box::new(Expr::LocalGet(COUNTER_ID)),
                }),
            },
            binding_accumulate(elem_field(ARRAY_ID, Expr::LocalGet(COUNTER_ID))),
        ],
        None,
    );
    m.init.insert(
        1,
        Stmt::Let {
            id: OTHER_ID,
            name: "other".to_string(),
            ty: Type::Array(Box::new(Type::Named("Node".to_string()))),
            mutable: false,
            init: Some(Expr::Array(Vec::new())),
        },
    );
    let ir = emit(&m);
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "a body reading two different arrays must decline the clone"
    );
}

/// The full positive contract in one helper: the clone was admitted, the
/// deref block cond_brs INTO it, and its fast region carries neither a call
/// nor any element-read/field-diamond runtime symbol. Stronger than the
/// [`CLONE_LABELS`] presence checks alone — an emitted-but-deleted clone
/// passes those (#7690's failure shape).
fn assert_clone_fires_call_free(ir: &str, what: &str) {
    for label in CLONE_LABELS {
        assert!(
            ir.contains(label),
            "{what}: expected the element-shape clone, but `{label}` is absent \
             — the matcher declined"
        );
    }
    assert_fast_clone_is_entered(ir);
    let fast = fast_clone_slice(ir);
    assert!(
        !fast.contains(" call "),
        "{what}: the fast clone must be call-free; found a call in:\n{fast}"
    );
    assert!(
        !fast.contains("js_array_get_f64") && !fast.contains("js_array_get_index_or_string"),
        "{what}: the element-read tier must be gone from the fast clone"
    );
    assert!(
        !fast.contains("js_object_get_field_by_name_f64")
            && !fast.contains("js_typed_feedback_class_field_get_guard")
            && !fast.contains("js_number_coerce"),
        "{what}: the by-name field diamond must be gone from the fast clone"
    );
}

#[test]
fn an_element_binding_indexed_by_a_non_counter_declines() {
    // `const r = keep[0]` is not the induction read; the preheader's range
    // argument covers only `arr[counter]`.
    let ir = emit(&element_shape_module(
        vec![
            Stmt::Let {
                id: BINDING_ID,
                name: "r".to_string(),
                ty: Type::Named("Node".to_string()),
                mutable: false,
                init: Some(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(ARRAY_ID)),
                    index: Box::new(Expr::Integer(0)),
                }),
            },
            binding_accumulate(binding_field("v")),
        ],
        None,
    ));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "a non-counter-indexed element binding must decline the clone"
    );
}

#[test]
fn element_binding_form_through_a_parameter_gets_the_clone() {
    // #7766's case (A), binding spelling: `function total(ps: Node[])`. The
    // array's provenance is the CALLER's — no static rule can prove it — and
    // the clone must be admitted anyway, because its preheader establishes
    // the element shape at run time and the declared type only names the
    // class id to check against.
    const PS_ID: u32 = 21;
    const F_SUM_ID: u32 = 22;
    const F_COUNTER_ID: u32 = 23;
    const F_BINDING_ID: u32 = 24;
    let mut m = Module::new("element_shape_loop.ts");
    m.classes = vec![node_class(None)];
    m.functions = vec![perry_hir::Function {
        id: 900,
        name: "total".to_string(),
        type_params: Vec::new(),
        params: vec![perry_hir::Param {
            id: PS_ID,
            name: "ps".to_string(),
            ty: Type::Array(Box::new(Type::Named("Node".to_string()))),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: F_SUM_ID,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(0.0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: F_COUNTER_ID,
                    name: "j".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(F_COUNTER_ID)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(PS_ID)),
                        property: "length".to_string(),
                        byte_offset: 0,
                    }),
                }),
                update: Some(Expr::Update {
                    id: F_COUNTER_ID,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![
                    Stmt::Let {
                        id: F_BINDING_ID,
                        name: "r".to_string(),
                        ty: Type::Named("Node".to_string()),
                        mutable: false,
                        init: Some(Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(PS_ID)),
                            index: Box::new(Expr::LocalGet(F_COUNTER_ID)),
                        }),
                    },
                    Stmt::Expr(Expr::LocalSet(
                        F_SUM_ID,
                        Box::new(Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(Expr::LocalGet(F_SUM_ID)),
                            right: Box::new(Expr::PropertyGet {
                                object: Box::new(Expr::LocalGet(F_BINDING_ID)),
                                property: "v".to_string(),
                                byte_offset: 0,
                            }),
                        }),
                    )),
                ],
            },
            Stmt::Return(Some(Expr::LocalGet(F_SUM_ID))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    m.init_kind = ModuleInitKind::Eager;
    let ir = emit(&m);
    assert_clone_fires_call_free(&ir, "parameter binding form");
}
