//! The monomorphic class-field GET tower has ONE exit.
//!
//! The tower in [`super::property_get`] used to emit four runtime call sites
//! per `this.field` / `obj.field` read — `js_typed_feedback_class_field_get_
//! guard` when the #5093 inline pre-check missed, `js_throw_type_error_
//! property_access` for a nullish receiver, and one or two
//! `js_object_get_field_by_name_f64` by-name lookups — spread over
//! `class_field_get.{fast,fallback,merge,throw_nullish,fallback_lookup}`. At
//! ~74 pre-RS4GC instructions and 13,636 sites in @babel/parser that arm alone
//! was 27 % of the module's IR, and because every call is a statepoint the
//! `.perry_gcmap` grew with it.
//!
//! Everything behind the pre-check now lives in `js_class_field_get_ic`, the
//! runtime helper the #5391 path-2 full outline already called. What must hold,
//! and why a label-presence check cannot show it:
//!
//! 1. **The inline hit path is still there and still REACHED.** A one-exit
//!    tower that also lost its pre-check would be a pure pessimisation, and
//!    every assertion below about "one call" would still pass. So: a `cond_br`
//!    out of `class_field_inline.deref` INTO `class_field_get.fast`, and a
//!    `load double` inside that block. (CLAUDE.md, "a gate must assert its
//!    subject was live".)
//! 2. **The miss arm is exactly one call.** Counted, not merely "the IC is
//!    mentioned": a regression that re-adds the by-name fallback beside the IC
//!    would keep every positive assertion true.
//! 3. **The retired symbols are not CALLED at this site.** Asserted as
//!    `call <ty> @name(`, never as a bare substring — all three are still
//!    declared in the module (and still emitted by other towers), so a
//!    substring test is satisfied by the `declare` line alone and can never
//!    fail. That exact trap is why the older assertions in
//!    `tests/typed_feedback.rs` were rewritten rather than deleted.
//! 4. **Both representations.** A `number` field (`require_raw_f64 = 1`) and an
//!    `any` field (boxed) take the same shape; the raw-f64 site must not grow a
//!    second by-name call back.

use crate::compile_module;
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Function, Module, ModuleInitKind, Param, Stmt};

const PARAM_ID: u32 = 3;

/// Blocks the tower is allowed to create, in the order it creates them.
const TOWER_BLOCKS: [&str; 4] = [
    "class_field_inline.deref",
    "class_field_inline.guardcall",
    "class_field_get.fast",
    "class_field_get.merge",
];

/// Blocks the four-exit tower created that must no longer exist.
const RETIRED_BLOCKS: [&str; 3] = [
    "class_field_get.fallback",
    "class_field_get.throw_nullish",
    "class_field_get.fallback_lookup",
];

/// Call FORMS (not bare names) for the runtime entries the tower stopped
/// emitting. Each is still declared in every module and still emitted by other
/// lowerings, so only the call form can distinguish "not called here".
const RETIRED_CALLS: [&str; 3] = [
    "call i32 @js_typed_feedback_class_field_get_guard(",
    "call void @js_throw_type_error_property_access(",
    "call double @js_object_get_field_by_name_f64(",
];

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

fn point_class(field_ty: Type) -> Class {
    Class {
        id: 101,
        name: "Point".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![field("x", field_ty)],
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

/// `function probe(p: Point) { return p.x }` — a bare `Named` parameter, which
/// is what routes to the generic `class_field_get.*` tower (#8033: the
/// numeric-specific `class_field_get_number.*` path needs a
/// `stable_local_type_proof` a parameter has not got).
fn probe_module(field_ty: Type) -> Module {
    let mut m = Module::new("class_field_get_shape.ts");
    m.classes = vec![point_class(field_ty)];
    m.functions = vec![Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: PARAM_ID,
            name: "p".to_string(),
            ty: Type::Named("Point".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::LocalGet(PARAM_ID)),
            property: "x".to_string(),
        }))],
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
    m
}

fn ir(field_ty: Type) -> String {
    String::from_utf8(
        compile_module(
            &probe_module(field_ty),
            super::class_field_barrier_tests::ir_opts(),
        )
        .expect("module compiles"),
    )
    .expect("LLVM IR should be UTF-8")
}

/// Body of the first block whose label line starts with `prefix`. Block labels
/// carry per-function numeric suffixes (`.fast.6`), so callers pass the stable
/// prefix; a label line is line-initial and ends in `:`, while branches and
/// phis that merely mention it are indented.
fn block_body<'a>(ir: &'a str, prefix: &str) -> Option<&'a str> {
    let needle = format!("\n{prefix}");
    let mut from = 0;
    while let Some(rel) = ir[from..].find(&needle) {
        let label_start = from + rel + 1;
        let line_end = label_start + ir[label_start..].find('\n')?;
        if ir[label_start..line_end].ends_with(':') {
            let rest = &ir[line_end + 1..];
            let end = match (rest.find("\n\n"), rest.find("\n}")) {
                (Some(a), Some(b)) => a.min(b),
                (a, b) => a.or(b).unwrap_or(rest.len()),
            };
            return Some(&rest[..end]);
        }
        from = line_end;
    }
    None
}

/// The `prefix.N:` label as rendered, e.g. `class_field_get.fast.6`.
fn block_label(ir: &str, prefix: &str) -> Option<String> {
    let needle = format!("\n{prefix}");
    let mut from = 0;
    while let Some(rel) = ir[from..].find(&needle) {
        let label_start = from + rel + 1;
        let line_end = label_start + ir[label_start..].find('\n')?;
        let line = &ir[label_start..line_end];
        if line.ends_with(':') {
            return Some(line.trim_end_matches(':').to_string());
        }
        from = line_end;
    }
    None
}

fn assert_one_exit(field_ty: Type, what: &str) {
    let ir = ir(field_ty);

    // (1) the pre-check exists and BRANCHES INTO the fast slot load.
    let deref = block_body(&ir, "class_field_inline.deref")
        .unwrap_or_else(|| panic!("{what}: no class_field_inline.deref block:\n{ir}"));
    let fast_label = block_label(&ir, "class_field_get.fast")
        .unwrap_or_else(|| panic!("{what}: no class_field_get.fast block:\n{ir}"));
    assert!(
        deref.contains("br i1 ") && deref.contains(&format!("label %{fast_label}")),
        "{what}: class_field_inline.deref does not branch into {fast_label}; \
         the inline hit path is dead:\n{deref}"
    );
    let fast = block_body(&ir, "class_field_get.fast")
        .unwrap_or_else(|| panic!("{what}: no class_field_get.fast body:\n{ir}"));
    assert!(
        fast.contains("load double"),
        "{what}: the fast block no longer loads the slot:\n{fast}"
    );
    assert!(
        !fast.contains("call "),
        "{what}: the fast block must stay call-free:\n{fast}"
    );

    // (2) the miss arm is EXACTLY one call.
    let guardcall = block_body(&ir, "class_field_inline.guardcall")
        .unwrap_or_else(|| panic!("{what}: no class_field_inline.guardcall block:\n{ir}"));
    let calls = guardcall.matches("call ").count();
    assert_eq!(
        calls, 1,
        "{what}: the miss arm must be ONE call, found {calls}:\n{guardcall}"
    );
    assert!(
        guardcall.contains("call double @js_class_field_get_ic("),
        "{what}: the miss arm's one call is not js_class_field_get_ic:\n{guardcall}"
    );

    // (3) the retired blocks and the retired CALL FORMS are gone.
    for block in RETIRED_BLOCKS {
        assert!(
            block_body(&ir, block).is_none(),
            "{what}: retired block {block} is still emitted:\n{ir}"
        );
    }
    for call in RETIRED_CALLS {
        assert!(
            !ir.contains(call),
            "{what}: retired call form `{call}` is still emitted:\n{ir}"
        );
    }

    // (4) the merge is a two-way phi (fast value, IC value) and nothing else.
    let merge = block_body(&ir, "class_field_get.merge")
        .unwrap_or_else(|| panic!("{what}: no class_field_get.merge block:\n{ir}"));
    let phis: Vec<&str> = merge.lines().filter(|l| l.contains("phi double")).collect();
    assert_eq!(phis.len(), 1, "{what}: merge is not a single phi:\n{merge}");
    assert_eq!(
        phis[0].matches('[').count(),
        2,
        "{what}: merge phi does not have exactly two incoming edges (fast, IC):\n{}",
        phis[0]
    );

    // (5) the tower creates exactly four blocks.
    for block in TOWER_BLOCKS {
        assert!(
            block_body(&ir, block).is_some(),
            "{what}: tower block {block} missing:\n{ir}"
        );
    }
}

#[test]
fn boxed_class_field_get_has_one_exit() {
    assert_one_exit(Type::Any, "any-typed field");
}

#[test]
fn raw_f64_class_field_get_has_one_exit() {
    // `number` makes `requires_raw_f64` true, which used to emit a SECOND
    // by-name fallback record/call pair. The IC takes `require_raw_f64` as an
    // argument instead, so the site stays at one call.
    assert_one_exit(Type::Number, "number-typed field");
}

#[test]
fn the_ic_call_carries_the_guard_operands() {
    // The helper runs the same guard the inline miss arm used to call, so it
    // must receive the same seven operands in the same order — site id,
    // receiver, expected class id, expected shape id, key, field index,
    // require_raw_f64. A dropped operand compiles and silently guards on the
    // wrong thing.
    let ir = ir(Type::Number);
    let guardcall = block_body(&ir, "class_field_inline.guardcall")
        .expect("guardcall block")
        .to_string();
    let line = guardcall
        .lines()
        .find(|l| l.contains("@js_class_field_get_ic("))
        .expect("the IC call line");
    let args = line
        .rsplit_once("@js_class_field_get_ic(")
        .expect("call args")
        .1;
    let args = args.rsplit_once(')').expect("closing paren").0;
    let tys: Vec<&str> = args
        .split(", ")
        .map(|a| a.split_whitespace().next().unwrap_or(""))
        .collect();
    assert_eq!(
        tys,
        vec!["i64", "double", "i32", "i32", "i64", "i32", "i32"],
        "IC call signature drifted from the guard's operand list:\n{line}"
    );
}
