//! #7715 (B3): the guarded array-ELEMENT store's remembered-set write barrier
//! sits behind a live test of the stored VALUE, and then of the parent's
//! generation.
//!
//! The subject is the tail of
//! [`super::index_set_guarded::emit_guarded_inbounds_array_store`] — the inline
//! in-bounds arm `this.vals[i] = v` lowers to — and its emitter
//! [`super::write_barrier::emit_write_barrier_slot_value_and_generation_tested`].
//!
//! ## Why the value test is the outer one here
//!
//! `gc-handoff/apps/pipeline.ts`'s `Registry.set` runs `this.vals[i] = v`
//! 1.44 M times. Measured with `PERRY_GC_TRACE=1`, the program makes
//! **1 265 933 write-barrier calls of which 1 265 925 (99.999%) exit at
//! `non_pointer_child_skips`** — the stored value is a number and the call does
//! nothing at all — while `parent_not_old_skips` is **0**, because the
//! container's backing array is long-lived. So on this shape the parent gate
//! (#7511/#7871) would skip nothing and the value test skips everything, which
//! is the opposite of the array PUSH the parent gate was written for.
//!
//! Nesting, rather than one fused `and`, is load-bearing:
//! `emit_may_carry_heap_pointer_check` is pure register arithmetic on the
//! stored bits, while `emit_parent_may_need_remembering_check` performs a
//! `monotonic` load of `@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT`, which
//! LLVM may not hoist out of the loop. Fusing would charge that non-hoistable
//! load to every numeric store — the exact traffic this exists to make free.
//!
//! ## What these tests pin
//!
//! Same three obligations as `class_field_barrier_tests`, on the new site:
//!
//! 1. the value gate is **REACHED** — a `cond_br` INTO `…barrier.maybe`, not
//!    merely a block with that name;
//! 2. its condition **is** the pointer-bearing predicate, and the inner block's
//!    condition **is** the live header + incremental-count disjunction, proved
//!    by walking the def chain rather than by finding the instructions nearby;
//! 3. the barrier is on the TAKEN edge of both and `js_write_barrier_slot` is
//!    still emitted inside — this is a guard, never an elision.

use super::class_field_barrier_tests::{
    assert_default_barrier_env_not_disabled, block_body, branch_into_block, def_of, ir_opts,
    operand,
};
use crate::compile_module;
use perry_hir::types::Type;
use perry_hir::{
    Class, ClassField, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt, UpdateOp,
};

/// The blocks that exist only when the #7715 gate was emitted. `idxset.recv_prop`
/// is the `block_prefix` `expr/index_set.rs` passes for a property receiver.
const VALUE_GATE_BLOCK: &str = "idxset.recv_prop.barrier.maybe";
const BARRIER_BLOCK: &str = "idxset.recv_prop.barrier.";
const BARRIER_CALL: &str = "call void @js_write_barrier_slot";
const INCREMENTAL_GLOBAL: &str = "@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT";

const RECV_ID: u32 = 11;
const IDX_ID: u32 = 12;
const VAL_ID: u32 = 13;

fn any_field(name: &str) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty: Type::Array(Box::new(Type::Any)),
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

/// `set(v) { for (let i = 0; i < 8; i++) this.vals[i] = v }` — `Registry.set`'s
/// hot statement, minus the key compare.
///
/// Three properties of the fixture are load-bearing and each was arrived at by
/// watching the emitted IR take a different path without it:
///
/// * the receiver is a **property**, not a stack local, which is what routes
///   the store through `emit_guarded_inbounds_array_store` instead of
///   `lower_index_set_fast` (the latter needs a slot to write a realloc'd head
///   back to);
/// * the index is a **`for`-loop counter seeded from an integer literal**, not
///   a `number` parameter. `numeric_index_needs_runtime_key` sends any numeric
///   index WITHOUT an integer-array-index proof to
///   `js_typed_feedback_array_set_index_or_string` — a `let k = 1.5` must write
///   the property `"1.5"` rather than truncate — and a `Type::Number` param has
///   no such proof;
/// * `v: Any` is what leaves the stored value's pointer-ness undecidable
///   statically and puts the store on the live-test tier at all.
fn setter() -> Function {
    Function {
        id: 91,
        name: "set".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: VAL_ID,
            name: "v".to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Void,
        body: vec![Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: IDX_ID,
                name: "i".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            })),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(IDX_ID)),
                right: Box::new(Expr::Integer(8)),
            }),
            update: Some(Expr::Update {
                id: IDX_ID,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body: vec![Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::This),
                    property: "vals".to_string(),
                    byte_offset: 0,
                }),
                index: Box::new(Expr::LocalGet(IDX_ID)),
                value: Box::new(Expr::LocalGet(VAL_ID)),
            })],
        }],
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

fn bag_class() -> Class {
    Class {
        id: 3,
        name: "Bag".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![any_field("vals")],
        constructor: None,
        methods: vec![setter()],
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
    let mut m = Module::new("index_set_barrier.ts");
    m.classes = vec![bag_class()];
    // The class method is what carries the subject; the init only has to make
    // the class reachable, so it allocates one instance and nothing else.
    m.init = vec![Stmt::Let {
        id: RECV_ID,
        name: "b".to_string(),
        ty: Type::Named("Bag".to_string()),
        mutable: false,
        init: Some(Expr::New {
            class_name: "Bag".to_string(),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        }),
    }];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn ir() -> String {
    String::from_utf8(compile_module(&probe_module(), ir_opts()).expect("module compiles"))
        .expect("LLVM IR should be UTF-8")
}

/// (1) + (2), outer half: the VALUE gate exists, is branched into, and its
/// condition is the pointer-bearing predicate rather than a constant.
///
/// The def-chain walk is what separates this from a label-presence check: a
/// sabotage that hard-wires `br i1 true` while leaving the (now dead) predicate
/// instructions in the block passes a "does the block contain an lshr" test and
/// fails this one.
#[test]
fn the_element_store_barrier_sits_behind_a_live_value_test() {
    assert_default_barrier_env_not_disabled();
    let ir = ir();
    let (branch, body) = branch_into_block(&ir, VALUE_GATE_BLOCK).unwrap_or_else(|| {
        panic!(
            "no `br i1 ..., label %{VALUE_GATE_BLOCK}` — the #7715 value gate was \
             never REACHED, so every numeric element store still pays the \
             remembered-set call (1.27 M of them on pipeline.ts):\n{ir}"
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
        "the value gate's condition is the constant `{cond}` — the branch \
         cannot fail, so the barrier is either always taken (no win) or NEVER \
         taken (a stranded child on the next minor GC): {branch}"
    );
    // cond = or(or(or(is_pointer, is_string), is_bigint), is_raw_addr)
    let or_instr = def_of(&body, cond).unwrap_or_else(|| {
        panic!("the value gate's condition {cond} is not defined in the branching block:\n{body}")
    });
    assert!(
        or_instr.starts_with("or i1 "),
        "the value gate's condition is `{or_instr}`, not the disjunction over \
         the heap-bearing NaN-box tags:\n{body}"
    );
    // Every heap tag the runtime's `decode_heap_addr` accepts must be compared
    // against. Dropping one is the direction that STRANDS a child, so the whole
    // comparand set is pinned, not just its shape.
    for tag in ["32765", "32767", "32762"] {
        assert!(
            body.contains(&format!(", {tag}\n")) || body.contains(&format!(", {tag}")),
            "the value gate never compares against tag {tag}; a value carrying \
             it would skip the barrier while the runtime would have decoded a \
             heap address from it:\n{body}"
        );
    }
    assert!(
        body.contains("lshr i64") && body.contains(", 48"),
        "the value gate does not derive the top 16 bits of the stored value, \
         so it is testing something other than the NaN-box tag:\n{body}"
    );

    let successors: Vec<&str> = branch
        .split("label %")
        .skip(1)
        .map(|part| part.split([',', ' ']).next().unwrap())
        .collect();
    assert_eq!(successors.len(), 2, "expected a two-way branch: {branch}");
    assert!(
        successors[0].starts_with(VALUE_GATE_BLOCK),
        "the barrier is on the FALSE edge of the value test — a pointer store \
         would skip the barrier and a numeric one would take it, which is \
         exactly backwards: {branch}"
    );
}

/// (2), inner half: the block the value gate branches into is the #7511 parent
/// generation test, unchanged — `GC_FLAG_TENURED` off a live header load, OR
/// the incremental-cycle count.
///
/// The incremental clause is not decoration. Skipping the call also skips
/// `barrier_child_prologue`'s SATB shading, which is not a generational
/// question; `gc::tests::inline_generation_gate_contract` is the runtime-side
/// half of the same obligation.
#[test]
fn the_value_gate_branches_into_the_parent_generation_test() {
    assert_default_barrier_env_not_disabled();
    let ir = ir();
    let body = block_body(&ir, VALUE_GATE_BLOCK)
        .unwrap_or_else(|| panic!("no `{VALUE_GATE_BLOCK}` block body:\n{ir}"));
    let branch = body
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("br i1 "))
        .unwrap_or_else(|| {
            panic!("`{VALUE_GATE_BLOCK}` does not end in a two-way branch:\n{body}")
        });

    let cond = branch
        .strip_prefix("br i1 ")
        .and_then(|rest| rest.split(',').next())
        .map(str::trim)
        .unwrap_or("");
    assert!(
        cond.starts_with('%'),
        "the parent gate's condition is the constant `{cond}`: {branch}"
    );
    let or_instr = def_of(&body, cond).unwrap_or_else(|| {
        panic!("the parent gate's condition {cond} is not defined here:\n{body}")
    });
    assert!(
        or_instr.starts_with("or i1 "),
        "the parent gate's condition is `{or_instr}`, not the disjunction of \
         the generational and incremental clauses:\n{body}"
    );
    let tenured_cmp_reg = operand(or_instr, 0).expect("or lhs");
    let incremental_cmp_reg = operand(or_instr, 1).expect("or rhs");

    let tenured_cmp = def_of(&body, &tenured_cmp_reg).unwrap_or_default();
    assert!(
        tenured_cmp.starts_with("icmp ne i8 ") && tenured_cmp.ends_with(", 0"),
        "the generational clause is `{tenured_cmp}`, not `gc_flags & TENURED != 0`:\n{body}"
    );
    let mask_reg = operand(tenured_cmp, 0).expect("icmp lhs");
    let mask = def_of(&body, &mask_reg).unwrap_or_default();
    assert!(
        mask.starts_with("and i8") && mask.ends_with(", 32"),
        "the generational clause masks `{mask}` rather than GC_FLAG_TENURED (0x20):\n{body}"
    );

    let incremental_cmp = def_of(&body, &incremental_cmp_reg).unwrap_or_default();
    assert!(
        incremental_cmp.starts_with("icmp ne i32 ") && incremental_cmp.ends_with(", 0"),
        "the incremental clause is `{incremental_cmp}`:\n{body}"
    );
    let count_reg = operand(incremental_cmp, 0).expect("icmp lhs");
    let count_load = def_of(&body, &count_reg).unwrap_or_default();
    assert!(
        count_load.contains(INCREMENTAL_GLOBAL) && count_load.starts_with("load atomic i32"),
        "the incremental clause does not read {INCREMENTAL_GLOBAL} atomically; \
         skipping the barrier also skips SATB shading:\n{body}"
    );
}

/// Body of the `idxset.recv_prop.barrier.<N>` block — the innermost one, whose
/// label is the prefix followed by DIGITS.
///
/// A plain prefix match cannot be used: `idxset.recv_prop.barrier.maybe.<N>`
/// and `idxset.recv_prop.barrier.done.<N>` share the prefix, and `block_body`
/// returns the first match, so the assertion below would read the gate block
/// and report the call missing from it. (That is exactly how this test first
/// failed.)
fn numbered_barrier_block_body(ir: &str) -> Option<String> {
    ir.lines()
        .filter(|line| !line.starts_with(char::is_whitespace))
        .filter_map(|line| line.trim_end().strip_suffix(':'))
        .find(|label| {
            label
                .strip_prefix(BARRIER_BLOCK)
                .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
        })
        .and_then(|label| block_body(ir, label))
}

/// (3) The boundary: a guard, not an elision. A tenured array holding a pointer
/// store still reaches `js_write_barrier_slot`, in the block both gates branch
/// into. A test that asserted the call ABSENT would be pinning a stranded child.
#[test]
fn the_gated_element_store_still_reaches_the_barrier_call() {
    assert_default_barrier_env_not_disabled();
    let ir = ir();
    let barrier_body = numbered_barrier_block_body(&ir)
        .unwrap_or_else(|| panic!("no `{BARRIER_BLOCK}<N>` block body:\n{ir}"));
    assert!(
        barrier_body.contains(BARRIER_CALL),
        "js_write_barrier_slot was ELIDED rather than gated — a tenured array \
         would publish an old->young edge nobody records:\n{barrier_body}"
    );
    assert!(
        ir.contains("call void @js_gc_write_barriers_emitted(i32 1)"),
        "the module must still declare to the runtime that generated barriers \
         exist — the remembered set's arming protocol reads this:\n{ir}"
    );
}
