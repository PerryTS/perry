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
    assert_default_barrier_env_not_disabled, def_of, ir_opts, operand,
};
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, ModuleInitKind, Param, Stmt};

/// The static-key store IC (`expr/put_value_store_ic.rs`): the block holding
/// THE shape compare, and the block holding the slot store.
const HIT: &str = "put.pic.token";
const HIT_STORE: &str = "put.pic.hit.store";
/// Charter step 5 (P2c): the store check between the admission and the store.
const HIT_REP: &str = "put.pic.hit.rep";
/// The pointer-bearing arm. The IC passes the `"put.pic"` stem precisely so an
/// assertion about THIS site cannot be satisfied by a class-field store
/// elsewhere in the same module.
const BOOKKEEPING: &str = "put.pic.gc_bookkeeping";
/// Where a value that is not a plain double is classified.
const CLASSIFY: &str = "put.pic.classify";
/// The non-pointer (NaN-boxed tag) arm the pointer test's false edge enters.
const SCALAR: &str = "put.pic.scalar.tagged";
/// The string alias demotion, gated on the STRING tag inside the pointer arm.
const STRING_ALIAS: &str = "put.pic.string_alias";
const BARRIER: &str = "put.pic.barrier";

/// Every runtime helper gets a `declare` line whether or not it is called, so
/// every one of these must be matched in CALL form or the assertion is
/// vacuous — `ir.contains("js_write_barrier_slot")` is true of a module that
/// never emits a barrier.
const ADDREF: &str = "call void @js_string_addref_if_heap_string(";
/// The trailing `(` is what separates this from `..._aware(`.
const NOTE: &str = "call void @js_gc_note_slot_layout(";
const NOTE_AWARE: &str = "call void @js_gc_note_slot_layout_aware(";
const BARRIER_CALL: &str = "call void @js_write_barrier_slot";

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
/// A literal key plus `target == receiver` is the static-key store IC's
/// admission (`expr/put_value_store_ic.rs`), whatever the RHS. What the two
/// callers vary is the RHS: a runtime value is classified from its bits, and
/// only an SSA constant is classified at compile time.
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

/// #8185: the census (`barrier_stem_census_tests`) runs its uniform floor on
/// this file's pointer-possible probe — same fixture, distinct module name.
pub(super) fn census_put_pic_ir() -> String {
    write_pic_ir("census_put_pic", Expr::LocalGet(VALUE))
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

/// The `br i1` whose TRUE target is the block `stem` names EXACTLY (the stem
/// or the stem plus a numeric suffix), and the body of the block holding it.
/// A substring match would take `put.pic.gc_bookkeeping.done.N` for
/// `put.pic.gc_bookkeeping` — the empty join instead of the guarded arm.
fn branch_into_block(ir: &str, stem: &str) -> Option<(String, String)> {
    let mut current_body: Vec<&str> = Vec::new();
    for line in ir.lines() {
        let trimmed = line.trim_end();
        if !line.starts_with(char::is_whitespace) && trimmed.ends_with(':') {
            current_body.clear();
            continue;
        }
        let t = trimmed.trim_start();
        if t.starts_with("br i1 ") {
            if let Some(target) = operand(t, 1) {
                if label_is(target.trim_start_matches('%'), stem) {
                    return Some((trimmed.to_string(), current_body.join("\n")));
                }
            }
        }
        current_body.push(line);
    }
    None
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

/// #8184, carried onto the static-key store IC: the store is unconditional,
/// and every piece of GC bookkeeping sits behind a LIVE test of the bits
/// actually stored — never a compile-time claim about the RHS.
///
/// The value is classified cheapest-first: a plain double leaves from the
/// store block; everything else reaches `put.pic.classify`, whose
/// `emit_may_carry_heap_pointer_check` sends pointer-bearing bits to the
/// bookkeeping arm. Each assertion below fails under a specific sabotage:
/// a dropped disjunct, a hard-wired branch, dead IR, a missing call.
#[test]
fn static_write_pic_guards_its_bookkeeping_behind_a_live_pointer_test() {
    assert_default_barrier_env_not_disabled();
    let ir = write_pic_ir("write_pic_pointer_possible", Expr::LocalGet(VALUE));

    let hit = block(&ir, HIT)
        .unwrap_or_else(|| panic!("the store IC's shape-compare block must exist:\n{ir}"));
    let hit_store = block(&ir, HIT_STORE)
        .unwrap_or_else(|| panic!("the store IC's store block must exist:\n{ir}"));
    let classify = block(&ir, CLASSIFY)
        .unwrap_or_else(|| panic!("the store IC's value-classification block must exist:\n{ir}"));

    // (a) Every hit reaches the STORE block and stores there, before any
    // classification. A path that skipped the store would be a dropped write.
    assert!(
        hit_store
            .lines()
            .any(|l| l.trim().starts_with("store double")),
        "the store block must perform the slot store:\n{hit_store}"
    );

    // (b) None of the bookkeeping calls is unconditional.
    for helper in [ADDREF, NOTE, BARRIER_CALL] {
        assert!(
            !hit.contains(helper) && !hit_store.contains(helper) && !classify.contains(helper),
            "#8184: `{helper}` must never be UNCONDITIONAL on the store path:\n\
             hit:\n{hit}\nstore:\n{hit_store}\nclassify:\n{classify}"
        );
    }
    assert!(
        !ir.contains(NOTE_AWARE),
        "the scalar-aware note belongs to the emitter #8184 replaced:\n{ir}"
    );

    // (c) The bookkeeping arm is REACHED, from the classification block, by a
    // live branch whose false edge is the non-pointer (tagged) arm.
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
        label_is(&false_target, SCALAR),
        "the FALSE edge must enter the non-pointer arm, got %{false_target}: {branch}"
    );
    let scalar =
        block(&ir, SCALAR).unwrap_or_else(|| panic!("the non-pointer arm must exist:\n{ir}"));
    for helper in [ADDREF, BARRIER_CALL] {
        assert!(
            !scalar.contains(helper),
            "GC_STORE_AUDIT(POINTER_FREE): the non-pointer arm must not reach {helper}:\n{scalar}"
        );
    }

    // (d) The condition IS `emit_may_carry_heap_pointer_check`, proved by
    // walking the def chain in the classification block: `or(or-chain of the
    // pointer / string / bigint tags, and(top16 == 0, bits >= floor))`. A
    // dropped disjunct narrows the predicate — the direction that STRANDS a
    // child.
    let def = def_of(&classify, &cond).unwrap_or_else(|| {
        panic!("the guard condition %{cond} is not defined in the classify block:\n{classify}")
    });
    assert!(
        def.starts_with("or i1"),
        "the guard must be the tag-OR-bare-address disjunction, got `{def}`"
    );
    let tagged = operand(def, 0).expect("the disjunction has a tagged operand");
    let raw_addr = operand(def, 1).expect("the disjunction has a bare-address operand");
    assert!(
        def_of(&classify, &tagged).is_some_and(|d| d.starts_with("or i1")),
        "the tagged half must be an OR of the pointer / string / bigint tags:\n{classify}"
    );
    assert!(
        def_of(&classify, &raw_addr).is_some_and(|d| d.starts_with("and i1")),
        "the bare-address half must be `top16 == 0 AND bits >= floor`:\n{classify}"
    );

    // (e) The bits tested are the STORED double's bits: the classification's
    // tag extract reads a register the store block defines by bitcasting a
    // double (under RS4GC the stored value may be re-materialised per use, so
    // register identity with the `store double` operand is not asserted).
    let lshr: Vec<&str> = classify
        .lines()
        .map(str::trim)
        .filter(|l| l.contains("lshr i64") && l.trim_end().ends_with(", 48"))
        .collect();
    assert_eq!(
        lshr.len(),
        1,
        "expected exactly one `lshr i64 …, 48` (the tag extract) in the classify block, \
         got {lshr:?}:\n{classify}"
    );
    let tag_src = operand(lshr[0], 1).expect("`%t = lshr i64 %bits, 48` names its input");
    assert!(
        def_of(&hit_store, &tag_src).is_some_and(|d| d.starts_with("bitcast double")),
        "the tag must be extracted from the stored double's bits, bitcast in the store \
         block:\n{hit_store}"
    );

    // (f) The string demotion remains behind the STRING tag. Object layout
    // comes from ShapeId, so neither note call may appear.
    let (alias_branch, alias_pred) = branch_into_block(&ir, STRING_ALIAS).unwrap_or_else(|| {
        panic!("no `br i1 ..., label %{STRING_ALIAS}` — the string demotion is unreachable:\n{ir}")
    });
    assert!(
        alias_branch.trim_start().starts_with("br i1 %") && alias_pred.contains(", 32767"),
        "the string demotion must be gated on the STRING tag (0x7FFF):\n{alias_pred}"
    );
    let alias = block(&ir, STRING_ALIAS)
        .unwrap_or_else(|| panic!("the string-alias arm must exist:\n{ir}"));
    assert!(
        alias.contains(ADDREF),
        "a uniquely-owned string aliased into the slot must still be demoted:\n{alias}"
    );
    assert!(
        !ir.contains(NOTE) && !ir.contains(NOTE_AWARE),
        "object write IC still emits a layout note:\n{ir}"
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
        .unwrap_or_else(|| panic!("the store IC's shape-compare block must exist:\n{ir}"));
    let hit_store = block(&ir, HIT_STORE)
        .unwrap_or_else(|| panic!("the store IC's store block must exist:\n{ir}"));
    assert!(
        hit_store
            .lines()
            .any(|l| l.trim().starts_with("store double")),
        "the numeric arm must still store:\n{hit_store}"
    );
    for helper in [ADDREF, NOTE, BARRIER_CALL] {
        assert!(
            !hit.contains(helper) && !hit_store.contains(helper),
            "GC_STORE_AUDIT(POINTER_FREE): a value proven unable to carry GC pointer \
             bits must not reach {helper}:\n{hit_store}"
        );
    }
    for stem in [BOOKKEEPING, BARRIER, SCALAR, CLASSIFY] {
        assert!(
            !ir.contains(stem),
            "a store of an SSA-constant plain double must emit no guard at all — \
             `{stem}` means the constant was not recognised:\n{ir}"
        );
    }
}

/// The hit path's header reads are exactly the ones the shape does not
/// answer. The GC-kind byte (`-8`) and the forwarded flag (`-7`) are proved by
/// the ShapeId compare (rule 3; a forwarding stub's `+4` word is an address's
/// high half, below the ShapeId floor), so no `load i8` may precede the store —
/// the only byte load left on a hit is the barrier gate's TENURED test, which
/// lives in the pointer arm AFTER the store. Sabotage: re-inserting either
/// byte test into the guard chain turns this red.
#[test]
fn store_ic_hit_path_reads_no_gc_kind_or_forwarded_byte() {
    let ir = write_pic_ir("store_ic_header_reads", Expr::LocalGet(VALUE));
    for stem in [HIT, "put.pic.kind", HIT_STORE, CLASSIFY] {
        let body = block(&ir, stem)
            .unwrap_or_else(|| panic!("the store IC block `{stem}` must exist:\n{ir}"));
        assert!(
            !body.contains("load i8"),
            "`{stem}` reads a header BYTE on the hit path — the GC kind and the \
             forwarded flag are the ShapeId compare's to prove:\n{body}"
        );
    }
    let token = block(&ir, HIT).expect("token block");
    assert_eq!(
        token.matches("icmp eq i32").count(),
        1,
        "exactly ONE shape compare decides the hit:\n{token}"
    );
    assert!(
        token.contains("_packed_set"),
        "the shape compare reads the site's compact word:\n{token}"
    );
}

/// Charter step 3: the receiver kind and the Array-subclass numeric proof are
/// SHAPE facts (`perry_runtime::object::shapes::store_kind` — the runtime
/// publishes a word only for an `Ordinary` shape), so nothing per object
/// stands between the shape compare and the store check: the hit block loads
/// `_reserved` for the barrier only and branches straight to the rep block, with
/// no `class_id` read and no proof or ordinary-mark mask. Sabotage:
/// re-inserting any of the old per-object tests turns this red.
#[test]
fn store_ic_hit_reads_no_per_object_receiver_fact() {
    let ir = write_pic_ir("store_ic_receiver_kind", Expr::LocalGet(VALUE));
    assert!(
        block(&ir, "put.pic.class").is_none() && block(&ir, "put.pic.classless").is_none(),
        "no class / class-less admission block may exist:\n{ir}"
    );
    let kind = block(&ir, "put.pic.kind").unwrap_or_else(|| panic!("hit block:\n{ir}"));
    let term = kind.lines().last().unwrap_or("").trim().to_string();
    assert!(
        term.starts_with("br label %") && label_is(term.trim_start_matches("br label %"), HIT_REP),
        "a matched shape goes straight on to the store check: {term}\n{kind}"
    );
    assert!(
        !kind.contains("load i32") && !kind.contains(", 128") && !kind.contains(", 768"),
        "the hit reads no class id and tests no proof / ordinary-mark bit:\n{kind}"
    );
    // The store check (DESIGN §3.2): a word with neither flag set (bits 63
    // and 62 clear) stores at once. A word whose sign bit is set (an `F64`
    // lane) refuses a value whose exponent is all ones to the miss; a word
    // with bit 62 set (a ConstFn lane) admits only a closure of the site body.
    let rep = block(&ir, HIT_REP).unwrap_or_else(|| panic!("rep block:\n{ir}"));
    assert!(
        rep.contains("lshr i64") && rep.contains(", 62"),
        "the rep block must read the word flag bits (63, 62):\n{rep}"
    );
    let (_, r_true, r_false) = branch_targets(rep.lines().last().unwrap_or("").trim());
    assert!(
        label_is(&r_true, HIT_STORE) && label_is(&r_false, "put.pic.hit.lane"),
        "a plain word stores, a flagged word goes on to its lane check:\n{rep}"
    );
    let lane = block(&ir, "put.pic.hit.lane").unwrap_or_else(|| panic!("lane block:\n{ir}"));
    assert!(
        lane.contains("icmp slt i64"),
        "the lane block must test the word's F64 flag:\n{lane}"
    );
    let (_, l_true, l_false) = branch_targets(lane.lines().last().unwrap_or("").trim());
    assert!(
        label_is(&l_true, "put.pic.hit.f64") && label_is(&l_false, "put.pic.hit.constfn"),
        "the F64 flag picks the exponent test, otherwise the ConstFn check:\n{lane}"
    );
    let f64_lane = block(&ir, "put.pic.hit.f64").unwrap_or_else(|| panic!("f64 block:\n{ir}"));
    assert!(
        f64_lane.contains("9218868437227405312"),
        "the F64 lane must test the value's exponent:\n{f64_lane}"
    );
    let (_, f_true, f_false) = branch_targets(f64_lane.lines().last().unwrap_or("").trim());
    assert!(
        label_is(&f_true, "put.pic.miss") && label_is(&f_false, HIT_STORE),
        "a refused value misses, anything else stores:\n{f64_lane}"
    );
    assert_constfn_body_compare(&ir);

    // No other edge reaches the store.
    let into_store = ir
        .lines()
        .filter(|l| {
            let t = l.trim();
            t.starts_with("br ")
                && t.split("label %")
                    .skip(1)
                    .any(|x| label_is(x.split([',', ' ']).next().unwrap_or(""), HIT_STORE))
        })
        .count();
    assert_eq!(
        into_store, 3,
        "only the plain word, the F64 lane and the ConstFn check may enter the store:\n{ir}"
    );
}

/// The ConstFn value check admits a closure only when the closure's
/// `ClosureHeader::info` equals the site's body word (word 4 of the packed
/// record, loaded atomically). A compare of a value with itself, or one that
/// never reads the site word, admits a closure of ANY body. Sabotage:
/// comparing the body word with itself turns this red.
fn assert_constfn_body_compare(ir: &str) {
    let header = block(ir, "put.pic.constfn.header")
        .unwrap_or_else(|| panic!("the ConstFn value check block:\n{ir}"));
    let body_load = header
        .lines()
        .find(|l| l.contains("load atomic i64") && l.contains("monotonic"))
        .unwrap_or_else(|| panic!("the site body word is loaded atomically:\n{header}"));
    let body_reg = body_load
        .trim()
        .split(" = ")
        .next()
        .unwrap_or("")
        .to_string();
    let site_gep = header.lines().any(|l| {
        l.contains("getelementptr")
            && l.contains("_packed_set")
            && l.trim_end().ends_with(", i64 4")
    });
    assert!(
        site_gep,
        "the body word is word 4 of the site record:\n{header}"
    );
    let cmp = header
        .lines()
        .find(|l| l.contains("icmp eq i64") && l.contains(&body_reg))
        .unwrap_or_else(|| panic!("the body word must be compared:\n{header}"));
    let ops: Vec<&str> = cmp
        .trim()
        .split("icmp eq i64")
        .nth(1)
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .collect();
    assert!(
        ops.len() == 2 && ops[0] != ops[1],
        "the closure's info must be compared with the site body word, not with itself:\n{cmp}"
    );
}

/// The inline path is taken for ANY right-hand side. An RHS that allocates
/// (here a string concatenation, a collection point) used to disqualify the
/// site from the inline cache entirely; now the receiver is rooted across it
/// and re-read after it, and the site keeps its inline hit.
#[test]
fn store_ic_admits_a_collecting_rhs() {
    let ir = write_pic_ir(
        "store_ic_collecting_rhs",
        Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left: Box::new(Expr::LocalGet(VALUE)),
            right: Box::new(Expr::String("-suffix".to_string())),
        },
    );
    assert!(
        block(&ir, HIT_STORE).is_some(),
        "a collecting RHS must still reach the inline store:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_put_value_set_packed_miss("),
        "and keep the one miss entry:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_put_value_set_ic_miss(")
            && !ir.contains("call double @js_put_value_set_dyn_ic("),
        "no second store path may remain for a static key:\n{ir}"
    );
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
/// The tag is already known, but string alias demotion and the barrier remain.
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

    for helper in [ADDREF, BARRIER_CALL] {
        assert!(
            reference.contains(helper),
            "the reference store arm lost {helper}:\n{reference}"
        );
    }
    assert!(
        !reference.contains(NOTE_AWARE) && !reference.contains(NOTE),
        "object reference store still emits a layout note:\n{reference}"
    );
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

/// A stored value that can never be a closure (a literal, an operator's
/// primitive result) has nothing a ConstFn lane could admit, so the site emits
/// no ConstFn admission: a flagged lane takes the miss, as a failed admission
/// would. A value that may be a closure keeps it.
#[test]
fn store_ic_emits_no_constfn_admission_for_a_value_that_cannot_be_a_closure() {
    let constfn_blocks = [
        "put.pic.hit.constfn",
        "put.add.constfn",
        "put.pic.constfn.header",
    ];
    for (name, value) in [
        ("store_ic_constfn_number", Expr::Number(1.0)),
        ("store_ic_constfn_string", Expr::String("s".to_string())),
        (
            "store_ic_constfn_sum",
            Expr::Binary {
                op: perry_hir::BinaryOp::Add,
                left: Box::new(Expr::LocalGet(VALUE)),
                right: Box::new(Expr::Number(1.0)),
            },
        ),
    ] {
        let ir = write_pic_ir(name, value);
        for stem in constfn_blocks {
            assert!(
                block(&ir, stem).is_none(),
                "{name}: `{stem}` must not be emitted for a value that is never a closure:\n{ir}"
            );
        }
        assert!(
            block(&ir, "put.pic.hit.f64").is_some(),
            "{name}: the F64 lane check stays:\n{ir}"
        );
    }
    let ir = write_pic_ir("store_ic_constfn_any", Expr::LocalGet(VALUE));
    for stem in constfn_blocks {
        assert!(
            block(&ir, stem).is_some(),
            "a value that may be a closure keeps `{stem}`:\n{ir}"
        );
    }
}
