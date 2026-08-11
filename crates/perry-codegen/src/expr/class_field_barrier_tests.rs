//! #7864: the class-field store's remembered-set write barrier sits behind a
//! LIVE test of the parent's generation.
//!
//! The subject is the tail of
//! [`super::write_barrier::emit_jsvalue_slot_store_pointer_tested`]. #7511 put
//! that store's three bookkeeping calls behind one live test of the stored
//! VALUE ("does this publish a heap pointer at all"); this adds the other half
//! of the question the barrier itself asks ("is the parent old enough for
//! anyone to care"), which is where every object-literal constructor lives —
//! the instance was allocated in the nursery a few instructions earlier.
//!
//! ## What these tests have to pin, and why each direction
//!
//! A predicate that silently never fires still compiles and still prints the
//! right answer; the program just stays slow. A predicate wired the WRONG way
//! round also compiles and prints the right answer *until a minor GC lands in
//! the window*, and then strands a live child. So the census asserts three
//! things a label-presence check cannot separate:
//!
//! 1. **The gate is REACHED** — there is a `cond_br` INTO
//!    `class_field_set.barrier`, not merely a block with that name. (CLAUDE.md,
//!    "a gate must assert its subject was live"; #7690 is the precedent where
//!    an optimization was silently deleted while every label survived.)
//! 2. **The condition is the real one** — the block that branches loads
//!    `gc_flags` and masks `GC_FLAG_TENURED`, and reads
//!    `@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT`. Hard-wiring the claim
//!    (`false`, or a constant, or dropping the incremental disjunct) fails
//!    here. That is the sabotage this file is verified against.
//! 3. **The barrier is on the RIGHT edge and still REACHABLE** — the `true`
//!    successor is the barrier block, the `false` successor is the join, and
//!    `js_write_barrier_slot` is still emitted inside. This is a guard, never
//!    an elision: a receiver that IS tenured takes the call exactly as before,
//!    which is what makes the change a scheduling decision rather than a
//!    semantic one.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Function, Module, ModuleInitKind, Param, Stmt};

/// The block that exists only when the #7864 gate was emitted.
const BARRIER_BLOCK: &str = "class_field_set.barrier";
/// `GC_FLAG_TENURED` as the emitted `and i8` mask.
const TENURED_MASK: &str = "and i8";
const TENURED_VALUE: &str = ", 32";
const INCREMENTAL_GLOBAL: &str = "@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT";
const BARRIER_CALL: &str = "call void @js_write_barrier_slot";

/// These tests describe DEFAULT barrier emission. `PERRY_WRITE_BARRIERS=0`
/// removes every barrier and would make all of them vacuously "pass" the
/// negative half while the positive half fails for the wrong reason.
fn assert_default_barrier_env_not_disabled() {
    assert!(
        !matches!(
            std::env::var("PERRY_WRITE_BARRIERS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        "these tests describe DEFAULT barrier emission; PERRY_WRITE_BARRIERS must be unset or on"
    );
}

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

const PARAM_ID: u32 = 7;

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

/// `constructor(v) { this.v = v }` — the shape HIR synthesizes for every
/// closed-shape object literal (`lower/context.rs::mint_anon_shape_class`), so
/// this is `{ kind: "num", num: n }` after lowering, not a contrived fixture.
/// The parameter is `Any`, which is what makes the value's pointer-ness
/// undecidable statically and puts the store on the #7511 live-test tier at
/// all.
fn param_prologue_ctor() -> Function {
    Function {
        id: 90,
        name: "constructor".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: PARAM_ID,
            name: "v".to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Void,
        body: vec![Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: "v".to_string(),
            value: Box::new(Expr::LocalGet(PARAM_ID)),
        })],
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

fn boxed_class() -> Class {
    Class {
        id: 2,
        name: "Boxed".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![field("v", Type::Any)],
        constructor: Some(param_prologue_ctor()),
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

fn probe_module() -> Module {
    let mut m = Module::new("class_field_barrier.ts");
    m.classes = vec![boxed_class()];
    m.functions = vec![Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Named("Boxed".to_string()),
        body: vec![Stmt::Return(Some(Expr::New {
            class_name: "Boxed".to_string(),
            args: vec![Expr::Number(1.0)],
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
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
    m.init = vec![Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    })];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn ir() -> String {
    String::from_utf8(compile_module(&probe_module(), ir_opts()).expect("module compiles"))
        .expect("LLVM IR should be UTF-8")
}

/// The `br i1 %cond, label %class_field_set.barrier.N, label %...` line, plus
/// the body of the block that contains it.
///
/// Selected by following the LABEL DEFINITION of the branching block, not by
/// slicing text around the first mention of the barrier name — the first
/// mention IS the branch, and a slice starting there is empty.
fn branch_into_barrier(ir: &str) -> Option<(String, String)> {
    let mut current_body: Vec<&str> = Vec::new();
    for line in ir.lines() {
        let trimmed = line.trim_end();
        if !line.starts_with(char::is_whitespace) && trimmed.ends_with(':') {
            current_body.clear();
            continue;
        }
        let t = trimmed.trim_start();
        if t.starts_with("br i1 ") && t.contains(&format!("label %{BARRIER_BLOCK}")) {
            return Some((trimmed.to_string(), current_body.join("\n")));
        }
        current_body.push(line);
    }
    None
}

/// Body of the named block (label definition to its terminator, inclusive).
fn block_body(ir: &str, label_prefix: &str) -> Option<String> {
    let mut inside = false;
    let mut out: Vec<&str> = Vec::new();
    for line in ir.lines() {
        let trimmed = line.trim_end();
        if !line.starts_with(char::is_whitespace) && trimmed.ends_with(':') {
            if inside {
                break;
            }
            inside = trimmed.trim_end_matches(':').starts_with(label_prefix);
            continue;
        }
        if inside {
            out.push(line);
            if trimmed.trim_start().starts_with("br ") || trimmed.trim_start().starts_with("ret ") {
                break;
            }
        }
    }
    (inside && !out.is_empty()).then(|| out.join("\n"))
}

/// The instruction that defines `%reg` inside `body`, without its `%reg = `
/// prefix. `None` for a constant operand or a value defined elsewhere.
fn def_of<'a>(body: &'a str, reg: &str) -> Option<&'a str> {
    let needle = format!("{reg} = ");
    body.lines()
        .map(str::trim)
        .find(|l| l.starts_with(&needle))
        .map(|l| l[needle.len()..].trim())
}

/// The `i`th SSA operand (`%…`) of an instruction.
fn operand(instr: &str, i: usize) -> Option<String> {
    instr.match_indices('%').nth(i).map(|(pos, _)| {
        instr[pos..]
            .split(|c: char| c == ',' || c.is_whitespace() || c == ')')
            .next()
            .unwrap()
            .to_string()
    })
}

/// (1) + (2): the gate exists, is BRANCHED INTO, and its condition **is** the
/// live header test — proved by walking the def chain, not by looking for the
/// instructions somewhere nearby.
///
/// ★ The weaker version of this test (assert the block *contains* an
/// `and i8 …, 32` and the incremental global) passed a deliberate sabotage that
/// hard-wired the branch to `br i1 false` while leaving the now-dead predicate
/// instructions in the block. That is precisely the "gate that cannot fail"
/// shape CLAUDE.md catalogues, so the assertion walks:
///
///   cond → `or i1 %a, %b`
///   %a   → `icmp ne i8 %t, 0` → %t → `and i8 %f, 32` → %f → `load i8`
///   %b   → `icmp ne i32 %c, 0` → %c → atomic load of the incremental count
///
/// Any constant condition, any dropped disjunct, and any substitution of a
/// different predicate breaks a named link in that chain.
#[test]
fn the_class_field_barrier_sits_behind_a_live_parent_generation_test() {
    assert_default_barrier_env_not_disabled();
    let ir = ir();
    let (branch, body) = branch_into_barrier(&ir).unwrap_or_else(|| {
        panic!(
            "no `br i1 ..., label %{BARRIER_BLOCK}` — the #7864 gate was never \
             REACHED, so every object-literal field store still pays the \
             remembered-set call from the nursery:\n{ir}"
        )
    });

    let cond = branch
        .trim_start()
        .strip_prefix("br i1 ")
        .and_then(|rest| rest.split(',').next())
        .map(str::trim)
        .unwrap_or("");
    assert!(
        cond.starts_with('%'),
        "the gate's condition is the constant `{cond}` — the branch cannot \
         fail, so the barrier is either always taken (no win) or NEVER taken \
         (a stranded child on the next minor GC): {branch}"
    );
    let or_instr = def_of(&body, cond).unwrap_or_else(|| {
        panic!("the gate's condition {cond} is not defined in the branching block:\n{body}")
    });
    assert!(
        or_instr.starts_with("or i1 "),
        "the gate's condition is `{or_instr}`, not the disjunction of the \
         generational and incremental clauses:\n{body}"
    );
    let tenured_cmp_reg = operand(or_instr, 0).expect("or lhs");
    let incremental_cmp_reg = operand(or_instr, 1).expect("or rhs");

    // Clause 1: gc_flags & GC_FLAG_TENURED != 0, off a real i8 header load.
    let tenured_cmp = def_of(&body, &tenured_cmp_reg).unwrap_or_default();
    assert!(
        tenured_cmp.starts_with("icmp ne i8 ") && tenured_cmp.ends_with(", 0"),
        "the generational clause is `{tenured_cmp}`, not `gc_flags & TENURED != 0`:\n{body}"
    );
    let mask_reg = operand(tenured_cmp, 0).expect("icmp lhs");
    let mask = def_of(&body, &mask_reg).unwrap_or_default();
    assert!(
        mask.starts_with(TENURED_MASK) && mask.ends_with(TENURED_VALUE),
        "the generational clause masks `{mask}` rather than GC_FLAG_TENURED \
         (0x20) — a different bit would answer a different question:\n{body}"
    );
    let flags_reg = operand(mask, 0).expect("and lhs");
    assert!(
        def_of(&body, &flags_reg)
            .unwrap_or_default()
            .starts_with("load i8"),
        "the masked value is not loaded from the parent's GcHeader, so the \
         gate rests on something other than the live header:\n{body}"
    );

    // Clause 2: the incremental-cycle count. Dropping it would also drop
    // barrier_child_prologue's SATB shading, which is not a generational
    // question and must never be skipped while a cycle is live.
    let incremental_cmp = def_of(&body, &incremental_cmp_reg).unwrap_or_default();
    assert!(
        incremental_cmp.starts_with("icmp ne i32 ") && incremental_cmp.ends_with(", 0"),
        "the incremental clause is `{incremental_cmp}`:\n{body}"
    );
    let count_reg = operand(incremental_cmp, 0).expect("icmp lhs");
    assert!(
        def_of(&body, &count_reg)
            .unwrap_or_default()
            .contains(INCREMENTAL_GLOBAL),
        "the incremental clause does not read {INCREMENTAL_GLOBAL}; skipping \
         the barrier also skips SATB shading:\n{body}"
    );
    // The barrier must be on the TAKEN edge. A swapped `cond_br` compiles,
    // prints the right answer, and strands a child on the next minor GC.
    let successors: Vec<&str> = branch
        .split("label %")
        .skip(1)
        .map(|part| part.split([',', ' ']).next().unwrap())
        .collect();
    assert_eq!(successors.len(), 2, "expected a two-way branch: {branch}");
    assert!(
        successors[0].starts_with(BARRIER_BLOCK),
        "the barrier is on the FALSE edge — an untenured parent would take the \
         call and a tenured one would skip it, which is the failure this gate \
         exists to avoid: {branch}"
    );
    assert!(
        successors[1].starts_with("class_field_set.gc_bookkeeping.done"),
        "the not-needed edge must join at the bookkeeping continuation: {branch}"
    );
}

/// (3) The boundary: a guard, not an elision.
///
/// `js_write_barrier_slot` must still be emitted, inside the barrier block.
/// A tenured receiver — an object promoted between its allocation and this
/// store, or one written long after construction — reaches it exactly as
/// before. A test that asserted the call ABSENT would be pinning a stranded
/// child.
#[test]
fn the_gated_arm_still_reaches_the_barrier_call() {
    assert_default_barrier_env_not_disabled();
    let ir = ir();
    assert!(
        ir.contains(BARRIER_CALL),
        "js_write_barrier_slot was ELIDED rather than gated — a tenured parent \
         would publish an old->young edge nobody records:\n{ir}"
    );
    let barrier_body = block_body(&ir, BARRIER_BLOCK)
        .unwrap_or_else(|| panic!("no `{BARRIER_BLOCK}` block body:\n{ir}"));
    assert!(
        barrier_body.contains(BARRIER_CALL),
        "the barrier call left the block the gate branches into, so it is \
         reached under some OTHER condition than the parent's generation:\n\
         {barrier_body}"
    );
    assert!(
        ir.contains("call void @js_gc_write_barriers_emitted(i32 1)"),
        "the module must still declare to the runtime that generated barriers \
         exist — the remembered set's arming protocol reads this:\n{ir}"
    );
}
