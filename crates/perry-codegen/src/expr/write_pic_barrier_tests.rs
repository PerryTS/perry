//! #8184 / #8185 — the write ICs' GC bookkeeping, asserted where per-PR CI can
//! actually run it.
//!
//! # Why this file is under `src/` and not `tests/`
//!
//! `test.yml`'s per-PR `cargo-test` gate is `--lib --bins`. Nothing in
//! `crates/*/tests/*.rs` runs on a pull request unless the diff happens to
//! name that suite (`e2e-scoped`); otherwise it is nightly/tag only. #8183's
//! barrier assertions were written into
//! `crates/perry-codegen/tests/native_proof_regressions.rs`, so they gated
//! their own PR and would have gated no future one — including this one, which
//! moves a store on a GC-managed slot. They are moved here (#5960), and the
//! new #8184 assertions are written here from the start.
//!
//! # Why a static IR assertion is the ONLY evidence that counts here
//!
//! A deleted write barrier is invisible to every runtime probe. It corrupts
//! nothing at the store; it leaves the remembered set merely INCOMPLETE, and
//! turning that into an observable failure needs the parent tenured, the child
//! still young, a MINOR collection landing in that window, and that edge being
//! the only path to the child. `FORCE_EVACUATE` / `VERIFY_EVACUATION` verify
//! REWRITING, not REMEMBERING; `PERRY_GEN_GC=0` does not consult the
//! remembered set at all. Recorded in #8183: a release build with the barrier
//! deleted passes that entire matrix byte-identically, exit 0. The full
//! argument is `docs/src/internals/gc-rooting-invariant.md`, "The mirror
//! image".
//!
//! So these tests are written to fail under sabotage, not merely to describe:
//!
//! 1. **Presence** of all three bookkeeping calls in the pointer-capable arm.
//! 2. **Reachability** — a `br i1` INTO the arm. #8183's third sabotage left
//!    the arm behind as dead IR and initially PASSED a content-only check.
//! 3. **The condition is the real predicate**, walked by def-chain rather than
//!    matched nearby, so `br i1 true` with the dead instructions left in place
//!    fails.
//! 4. **The negative arm stays clean** — a value proven non-pointer must take
//!    a bare store, or the discriminator stopped discriminating and the
//!    optimization is measuring nothing.

use super::class_field_barrier_tests::{
    assert_default_barrier_env_not_disabled, branch_into_block, def_of, ir_opts, operand,
};
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, ModuleInitKind, Param, Stmt};

/// The hit block of the static write PIC — where the store itself lives.
const HIT: &str = "put.pic.hit";
/// #8184's guarded arm. `emit_jsvalue_slot_store_pointer_tested` names its
/// blocks from the `stem` its caller passes; the static write PIC passes
/// `"put.pic"` precisely so an assertion about THIS site cannot be satisfied
/// by a class-field store elsewhere in the same module.
const BOOKKEEPING: &str = "put.pic.gc_bookkeeping";
const BOOKKEEPING_DONE: &str = "put.pic.gc_bookkeeping.done";
const BARRIER: &str = "put.pic.barrier";

/// Every runtime helper gets a `declare` line whether or not it is called, so
/// every one of these must be matched in CALL form or the assertion is
/// vacuous — `ir.contains("js_write_barrier_slot")` is true of a module that
/// never emits a barrier.
const ADDREF: &str = "call void @js_string_addref_if_heap_string(";
/// The trailing `(` is what separates this from `..._aware(`.
const NOTE: &str = "call void @js_gc_note_slot_layout(";
const NOTE_AWARE: &str = "call void @js_gc_note_slot_layout_aware(";
const BARRIER_CALL: &str = "call void @js_write_barrier_slot(";

const OBJECT: u32 = 1;
const VALUE: u32 = 2;
const KEY: u32 = 3;

fn opts() -> CompileOptions {
    let mut o = ir_opts();
    o.is_entry_module = false;
    o
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

fn probe_module(name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Module {
    let mut m = Module::new(name);
    m.functions = vec![Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Any,
        body,
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

fn ir_of(module: Module) -> String {
    String::from_utf8(compile_module(&module, opts()).expect("module compiles"))
        .expect("LLVM IR should be UTF-8")
}

/// `function probe(object, value) { return object.x = <value> }`.
///
/// A literal key plus a call-free RHS plus `target == receiver` is exactly the
/// static write PIC's admission test (`lower_put_value_static_write_ic`). What
/// the two callers vary is the RHS, because `pointer_possible` — the whole
/// subject of #8184 — is a compile-time claim about it.
fn write_pic_ir(name: &str, value: Expr) -> String {
    ir_of(probe_module(
        name,
        vec![
            param(OBJECT, "object", Type::Any),
            param(VALUE, "value", Type::Any),
        ],
        vec![Stmt::Return(Some(Expr::PutValueSet {
            target: Box::new(Expr::LocalGet(OBJECT)),
            key: Box::new(Expr::String("x".to_string())),
            value: Box::new(value),
            receiver: Box::new(Expr::LocalGet(OBJECT)),
            strict: false,
        }))],
    ))
}

/// Emitted block labels carry a per-function numeric suffix
/// (`put.pic.gc_bookkeeping.60`), so a label matches a stem when it IS the
/// stem or the stem followed by `.` and digits only.
///
/// A plain `starts_with` cannot separate `put.pic.gc_bookkeeping` from
/// `put.pic.gc_bookkeeping.done`, and `.done` is the EMPTY block — so a prefix
/// match would satisfy every "the bookkeeping is here" assertion by inspecting
/// the wrong block, and every "it is not here" assertion for the wrong reason.
fn label_is(label: &str, stem: &str) -> bool {
    label == stem
        || label
            .strip_prefix(stem)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Body of the block whose label matches `stem` exactly, terminator included.
fn block(ir: &str, stem: &str) -> Option<String> {
    let mut inside = false;
    let mut out: Vec<&str> = Vec::new();
    for line in ir.lines() {
        let head = line.split(';').next().unwrap_or(line).trim_end();
        if !line.starts_with(char::is_whitespace) && head.ends_with(':') {
            if inside {
                break;
            }
            inside = label_is(head.trim_end_matches(':'), stem);
            continue;
        }
        if inside {
            out.push(line);
            let t = head.trim_start();
            if t.starts_with("br ")
                || t.starts_with("ret ")
                || t.starts_with("switch ")
                || t.starts_with("unreachable")
            {
                break;
            }
        }
    }
    (inside && !out.is_empty()).then(|| out.join("\n"))
}

/// The `%reg` operands of a `br i1 %c, label %t, label %f`, minus the `%`.
fn branch_targets(branch: &str) -> (String, String, String) {
    let cond = operand(branch, 0).expect("br i1 must name a condition register");
    let t = operand(branch, 1).expect("br i1 must name a true target");
    let f = operand(branch, 2).expect("br i1 must name a false target");
    (
        cond,
        t.trim_start_matches('%').to_string(),
        f.trim_start_matches('%').to_string(),
    )
}

/// #8184: the static write PIC's pointer-capable store no longer pays three
/// unconditional `gc-leaf` calls. It pays ONE inline test of the bits it is
/// about to store, and branches over all three.
///
/// Measured before: 118 instructions per write against the sibling dynamic-key
/// IC's 22 for the identical store, and +21.4% instructions on
/// `const v = f(); o.x = v` versus leaving the call inline (#8183).
#[test]
fn static_write_pic_guards_its_bookkeeping_behind_a_live_pointer_test() {
    assert_default_barrier_env_not_disabled();
    let ir = write_pic_ir("write_pic_pointer_possible", Expr::LocalGet(VALUE));

    let hit = block(&ir, HIT)
        .unwrap_or_else(|| panic!("the static write PIC's hit block must exist:\n{ir}"));

    // (a) The STORE stays unconditional and in the hit block. Only the
    // bookkeeping moved; a guard that also skipped the store would be a
    // dropped write, not an optimization.
    assert!(
        hit.lines().any(|l| l.trim().starts_with("store double")),
        "the hit block must still perform the slot store — only the bookkeeping \
         moved behind the guard, never the write itself:\n{hit}"
    );

    // (b) None of the three calls is unconditional any more. This is the
    // regression pin for #8184 itself: reverting to
    // `emit_jsvalue_slot_store_scalar_aware_on_block` puts all three back here.
    for helper in [ADDREF, NOTE, BARRIER_CALL] {
        assert!(
            !hit.contains(helper),
            "#8184: `{helper}` must no longer be UNCONDITIONAL in the PIC hit block:\n{hit}"
        );
    }
    assert!(
        !ir.contains(NOTE_AWARE),
        "the scalar-aware note belongs to the emitter #8184 replaced; its presence \
         means the static PIC is back on `emit_jsvalue_slot_store_scalar_aware_on_block`:\n{ir}"
    );

    // (c) The guarded arm is REACHED, on the right edge. An emitted block is
    // not a reached block — #8183's dead-IR near-miss is why this is asserted
    // before anything the block contains is believed.
    let (branch, _) = branch_into_block(&ir, BOOKKEEPING).unwrap_or_else(|| {
        panic!("no `br i1 ..., label %{BOOKKEEPING}` — the #8184 guard is dead IR:\n{ir}")
    });
    assert!(
        branch.trim_start().starts_with("br i1 %"),
        "the guard must be a LIVE test, not a hard-wired constant: {branch}"
    );
    let (cond, true_target, false_target) = branch_targets(&branch);
    assert!(
        label_is(&true_target, BOOKKEEPING),
        "the TRUE edge must enter the bookkeeping arm, got %{true_target}: {branch}"
    );
    assert!(
        label_is(&false_target, BOOKKEEPING_DONE),
        "the FALSE edge must skip straight to the join, got %{false_target}: {branch}"
    );

    // (d) The condition IS `emit_may_carry_heap_pointer_check`, proved by
    // walking the def chain rather than by finding its instructions somewhere
    // in the block. `or(or(or(ptr_tag, str_tag), bigint_tag), and(top16==0,
    // bits>=floor))` — a dropped disjunct narrows the predicate, which is the
    // direction that STRANDS a child.
    let def = def_of(&hit, &cond).unwrap_or_else(|| {
        panic!("the guard condition %{cond} is not defined in the hit block:\n{hit}")
    });
    assert!(
        def.starts_with("or i1"),
        "the guard must be the tag-OR-bare-address disjunction, got `{def}`"
    );
    let tagged = operand(def, 0).expect("the disjunction has a tagged operand");
    let raw_addr = operand(def, 1).expect("the disjunction has a bare-address operand");
    assert!(
        def_of(&hit, &tagged).is_some_and(|d| d.starts_with("or i1")),
        "the tagged half must itself be an OR of the pointer / string / bigint tags:\n{hit}"
    );
    assert!(
        def_of(&hit, &raw_addr).is_some_and(|d| d.starts_with("and i1")),
        "the bare-address half must be `top16 == 0 AND bits >= floor`:\n{hit}"
    );

    // (e) The bits tested are a JSValue's bits, taken in this block, and there
    // is exactly ONE such test — so the predicate above is that test and not a
    // second one left over from something else.
    //
    // Deliberately NOT asserted by register identity against the `store
    // double` operand: under RS4GC the rooted value is RE-MATERIALISED at
    // every use (`%rN.rs4p = load ptr addrspace(1)` → `inttoptr` → `bitcast`),
    // so the store, the guard and each helper argument legitimately name
    // different registers for the same value. An identity assertion here would
    // fail on correct IR, which is worse than not asserting it.
    let lshr: Vec<&str> = hit
        .lines()
        .map(str::trim)
        .filter(|l| l.contains("lshr i64") && l.trim_end().ends_with(", 48"))
        .collect();
    assert_eq!(
        lshr.len(),
        1,
        "expected exactly one `lshr i64 …, 48` (the tag extract) in the hit block, \
         got {lshr:?}:\n{hit}"
    );
    let tag_src = operand(lshr[0], 1).expect("`%t = lshr i64 %bits, 48` names its input");
    assert!(
        def_of(&hit, &tag_src).is_some_and(|d| d.starts_with("bitcast double")),
        "the tag must be extracted from a JSValue's bits, i.e. a double bitcast in \
         this block, not from an unrelated integer:\n{hit}"
    );

    // (f) The arm still does all three jobs.
    let book =
        block(&ir, BOOKKEEPING).unwrap_or_else(|| panic!("the bookkeeping arm must exist:\n{ir}"));
    assert!(
        book.contains(ADDREF),
        "a uniquely-owned string aliased into the slot must still be demoted:\n{book}"
    );
    assert!(
        book.contains(NOTE),
        "the pointer-bearing store must still record the slot's GC layout:\n{book}"
    );

    // (g) The barrier is reached, behind the #7871 parent-generation test, and
    // is still emitted.
    let (barrier_branch, barrier_pred) = branch_into_block(&ir, BARRIER).unwrap_or_else(|| {
        panic!("no `br i1 ..., label %{BARRIER}` — the write barrier is unreachable:\n{ir}")
    });
    assert!(
        barrier_branch.trim_start().starts_with("br i1 %"),
        "the barrier gate must be a LIVE header test: {barrier_branch}"
    );
    assert!(
        barrier_pred.contains("and i8") && barrier_pred.contains(", 32"),
        "the barrier gate must mask GC_FLAG_TENURED out of the parent's gc_flags:\n{barrier_pred}"
    );
    assert!(
        barrier_pred.contains("@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT"),
        "the barrier gate must keep its incremental-cycle disjunct — SATB shading \
         is not a generational question:\n{barrier_pred}"
    );
    let barrier_block =
        block(&ir, BARRIER).unwrap_or_else(|| panic!("the barrier arm must exist:\n{ir}"));
    assert!(
        barrier_block.contains(BARRIER_CALL),
        "the remembered-set write barrier must still be CALLED:\n{barrier_block}"
    );
}

/// The negative half, and it is not decoration: if the compile-time
/// non-pointer proof stopped being consulted, #8184 would look like a pure win
/// while having made the already-cheap arm expensive. A numeric RHS must still
/// emit nothing but the store.
#[test]
fn static_write_pic_keeps_a_bare_store_for_a_provably_non_pointer_value() {
    assert_default_barrier_env_not_disabled();
    let ir = write_pic_ir("write_pic_numeric", Expr::Number(1.0));

    let hit = block(&ir, HIT)
        .unwrap_or_else(|| panic!("the static write PIC's hit block must exist:\n{ir}"));
    assert!(
        hit.lines().any(|l| l.trim().starts_with("store double")),
        "the numeric arm must still store:\n{hit}"
    );
    for helper in [ADDREF, NOTE, BARRIER_CALL] {
        assert!(
            !hit.contains(helper),
            "GC_STORE_AUDIT(POINTER_FREE): a value proven unable to carry GC pointer \
             bits must not reach {helper}:\n{hit}"
        );
    }
    for stem in [BOOKKEEPING, BARRIER] {
        assert!(
            !ir.contains(stem),
            "a statically proven non-pointer store must emit no guard at all — \
             `{stem}` means lever D stopped firing:\n{ir}"
        );
    }
}

// ---------------------------------------------------------------------------
// #8183's dynamic-key IC assertions, moved here from
// `crates/perry-codegen/tests/native_proof_regressions.rs` so that per-PR CI
// runs them (#8185). Behaviour unchanged; only the location and the block
// helper differ.
// ---------------------------------------------------------------------------

/// `function probe(object, value) { let key = "x"; return object[key] = value }`
///
/// A MUTABLE string local as the key is what keeps this off the static PIC and
/// on the dynamic-key IC.
fn dyn_ic_reference_store_ir() -> String {
    ir_of(probe_module(
        "dyn_ic_reference_store",
        vec![
            param(OBJECT, "object", Type::Any),
            param(VALUE, "value", Type::Any),
        ],
        vec![
            Stmt::Let {
                id: KEY,
                name: "key".to_string(),
                ty: Type::String,
                mutable: true,
                init: Some(Expr::String("x".to_string())),
            },
            Stmt::Return(Some(Expr::PutValueSet {
                target: Box::new(Expr::LocalGet(OBJECT)),
                key: Box::new(Expr::LocalGet(KEY)),
                value: Box::new(Expr::LocalGet(VALUE)),
                receiver: Box::new(Expr::LocalGet(OBJECT)),
                strict: false,
            })),
        ],
    ))
}

/// #8108: a reference-tagged value stored through the inline dynamic-key write
/// IC takes a BARRIERED inline arm instead of leaving the inline path.
///
/// The reference arm is byte-for-byte the static write PIC's pre-#8184
/// pointer-capable store reached under strictly stronger conditions — the tag
/// is already known — so this test pins all three bookkeeping calls. Dropping
/// any one of them is the #5094 / #7511 family of silent-stranding bugs, and
/// none of them is visible to a runtime GC probe.
#[test]
fn dyn_ic_inline_store_barriers_a_reference_value() {
    assert_default_barrier_env_not_disabled();
    let ir = dyn_ic_reference_store_ir();

    let scalar = block(&ir, "put.dynic.store.scalar")
        .unwrap_or_else(|| panic!("the non-reference store arm must survive:\n{ir}"));
    let reference = block(&ir, "put.dynic.store.ref").unwrap_or_else(|| {
        panic!("a reference-tagged value must take an inline barriered arm:\n{ir}")
    });
    // An emitted block is not a reached block. Routing reference values back to
    // `put.dynic.slow` leaves this block behind as dead IR, which every
    // assertion below would happily inspect — so require the branch INTO it
    // before believing anything it contains.
    assert!(
        ir.lines()
            .any(|line| line.contains("br i1") && line.contains("%put.dynic.store.ref")),
        "the reference arm must be a branch target, not dead IR:\n{ir}"
    );

    for helper in [ADDREF, NOTE_AWARE, BARRIER_CALL] {
        assert!(
            reference.contains(helper),
            "the reference store arm must keep the full layout-note / string-alias / \
             write-barrier path; missing {helper}:\n{reference}"
        );
    }
    assert!(
        reference.contains("store double"),
        "the reference arm must still perform the slot store:\n{reference}"
    );

    // The scalar arm is the pre-#8108 IR: a bare store, no bookkeeping. A
    // barrier appearing here would mean the tag test stopped discriminating.
    assert!(
        scalar.contains("store double"),
        "the non-reference arm must still store:\n{scalar}"
    );
    for helper in [ADDREF, NOTE_AWARE, BARRIER_CALL] {
        assert!(
            !scalar.contains(helper),
            "GC_STORE_AUDIT(POINTER_FREE): the non-reference arm proved the value carries \
             no heap pointer, so it must not call {helper}:\n{scalar}"
        );
    }
}

/// #8108, the other half: admitting reference values inline must not cost the
/// semantic fallback. Every guard failure and every way miss still reaches
/// `js_put_value_set_dyn_ic`, which bottoms out at full `[[Set]]`.
#[test]
fn dyn_ic_inline_store_keeps_its_semantic_fallback_for_reference_values() {
    let ir = dyn_ic_reference_store_ir();

    assert!(
        ir.contains("call double @js_put_value_set_dyn_ic("),
        "the outlined helper must remain the miss path:\n{ir}"
    );
    // The arm is SELECTED by the value tag, not gated at entry: the branch into
    // the two store arms is what proves a reference value can reach the inline
    // store at all, rather than being diverted to the slow block above it.
    let selector = ir
        .lines()
        .find(|line| {
            line.contains("br i1")
                && line.contains("%put.dynic.store.scalar")
                && line.contains("%put.dynic.store.ref")
        })
        .unwrap_or_else(|| {
            panic!("the value tag must SELECT a store arm, not gate inline entry:\n{ir}")
        });
    assert!(
        selector.trim_start().starts_with("br i1"),
        "expected a conditional branch into the two store arms, got: {selector}"
    );
    // Entry must no longer reject on the value tag. The three tag compares
    // still exist (they build the selector), but the entry predicate is now
    // receiver-shaped plus the empty-way sentinel only.
    let entry = block(&ir, "put.dynic.guard")
        .unwrap_or_else(|| panic!("the receiver guard block must exist:\n{ir}"));
    assert!(
        entry.contains("call double @js_put_value_set_dyn_ic(") || ir.contains("%put.dynic.slow"),
        "the receiver guard must still fall through to the outlined helper:\n{ir}"
    );
}
