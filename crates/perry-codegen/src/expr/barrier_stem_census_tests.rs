//! #8185 — the write-barrier IR census: every stem-labelled barrier emitter
//! call site in this crate has a LIVE, uniformly-verified IR witness here.
//!
//! # Why this file exists
//!
//! A deleted write barrier passes every runtime probe (#8183, recorded in
//! #8185): it corrupts nothing at the store, it leaves the remembered set
//! merely incomplete, and `FORCE_EVACUATE` / `VERIFY_EVACUATION` /
//! `PERRY_GEN_GC=0` all verify the wrong property. The only detector is a
//! static assertion against the emitted LLVM IR. The per-stem test files
//! (`class_field_barrier_tests`, `index_set_barrier_tests`,
//! `write_pic_barrier_tests`, `array_push.rs::parent_gate_tests`) each pin
//! their own site DEEPLY; what none of them could do is ENUMERATE — a new
//! emitter call site with a new stem had no gate obliging it to bring a
//! witness. [`VERIFIED_BARRIER_STEMS`] is that obligation, made machine-
//! readable:
//!
//! * `scripts/gc_store_site_inventory.py` (the `lint` gate) scans every call
//!   to the stem-taking emitters, resolves each stem literal, and requires
//!   set-equality with this table in BOTH directions — a new stem with no
//!   witness fails lint, and a stale table entry fails lint.
//! * [`census_every_registered_stem_has_a_live_verified_witness`] compiles a
//!   probe per entry and runs the uniform floor below, so a table entry whose
//!   probe no longer reaches the tier fails `cargo test -p perry-codegen`.
//!
//! # Why it is under `src/` and not `tests/`
//!
//! Per-PR CI runs `cargo test --lib --bins`; everything in `crates/*/tests/`
//! is nightly/tag only. #8183's assertions started in `tests/` and gated
//! nothing after their own PR — #8189 moved them, and this file follows that
//! precedent (#5960).
//!
//! # The uniform floor (what [`verify_stem_ir`] asserts per stem)
//!
//! 1. **Reached**: a `cond_br` INTO `<stem>.barrier.<n>` — an emitted block is
//!    not a reached block (#7690, and #8183's dead-IR near-miss).
//! 2. **Real**: `call void @js_write_barrier_slot(` INSIDE that block, matched
//!    in call form (every helper gets a `declare` line whether or not it is
//!    called, so a whole-module `contains` is vacuous).
//! 3. **Live**: the branch condition walked by DEF-CHAIN back to the
//!    `GC_FLAG_TENURED` header load and the incremental-count atomic load.
//!    `br i1 true` with the dead predicate left in place fails here — nearby-
//!    text matching passed exactly that sabotage once (see
//!    `class_field_barrier_tests`' ★ note).
//! 4. Kind-specific outer gate: the value test for `ValueAndGenerationTested`
//!    (`<stem>.barrier.maybe.<n>`) and `PointerTestedStore`
//!    (`<stem>.gc_bookkeeping.<n>`), walked the same way, plus the
//!    unconditional `store double` staying OUTSIDE the guard.
//!
//! The floor is verified AGAINST SABOTAGE in this file: each of the four
//! surgeries below must turn every stem's verdict red on otherwise-pristine
//! IR, and the pristine IR must pass first, so a surgery that misses its
//! target cannot pass vacuously.
//!
//! # What this census cannot see (stated, not hidden)
//!
//! Barriers emitted UNGUARDED through the on-block emitters
//! (`emit_write_barrier_slot_on_block`, `emit_jsvalue_slot_store_*_on_block`)
//! carry no stem and are not enumerated here; their shared inner emitter is
//! pinned by `write_pic_barrier_tests`' dynamic-IC tests, but a caller that
//! passes `write_barrier_needed: false` where `true` was meant is a
//! parameterization bug this census does not catch.

use super::class_field_barrier_tests::{
    assert_default_barrier_env_not_disabled, def_of, ir_opts, operand,
};
use crate::compile_module;
use perry_hir::types::Type;
use perry_hir::{CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt, UpdateOp};

/// How a stem's barrier is gated, i.e. which emitter shape its witness must
/// match. Parsed by `scripts/gc_store_site_inventory.py` — keep entries on the
/// `("<stem>", StemKind::<Kind>)` single-line form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum StemKind {
    /// `emit_write_barrier_slot_generation_tested`: parent-generation gate
    /// only (the pushed value is always a pointer on this tier).
    GenerationTested,
    /// `emit_write_barrier_slot_value_and_generation_tested`: value test
    /// (`<stem>.barrier.maybe`) nested outside the generation gate.
    ValueAndGenerationTested,
    /// `emit_jsvalue_slot_store_pointer_tested`: unconditional store, then
    /// value-gated bookkeeping (`<stem>.gc_bookkeeping`) containing the
    /// generation-gated barrier.
    PointerTestedStore,
}

pub(super) const VERIFIED_BARRIER_STEMS: &[(&str, StemKind)] = &[
    ("apush", StemKind::GenerationTested),
    ("class_field_set", StemKind::PointerTestedStore),
    ("ctor_prologue", StemKind::ValueAndGenerationTested),
    ("idxset.inbounds", StemKind::ValueAndGenerationTested),
    ("idxset.recv_prop", StemKind::ValueAndGenerationTested),
    ("put.pic", StemKind::PointerTestedStore),
];

const BARRIER_CALL: &str = "call void @js_write_barrier_slot";
const INCREMENTAL_GLOBAL: &str = "@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT";

// ---------------------------------------------------------------------------
// IR navigation, numeric-suffix aware. Emitted labels carry a per-function
// uniquing suffix (`apush.barrier.21:`), and `<stem>.barrier.` is a PREFIX of
// `<stem>.barrier.done.` and `<stem>.barrier.maybe.` — so every match below
// requires the suffix after the prefix to be digits only. A plain substring
// or prefix test hands back the WRONG block, which is exactly the block the
// barrier is supposed to be in (see `array_push.rs::gated_barrier_block`).
// ---------------------------------------------------------------------------

/// Does `label` equal `<prefix><digits>` (prefix already ends with `.`)?
fn label_matches(label: &str, prefix: &str) -> bool {
    label
        .strip_prefix(prefix)
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The label a line defines, if it defines one (trailing `; preds` stripped).
fn label_of(line: &str) -> Option<&str> {
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let head = line.split(';').next().unwrap_or(line).trim_end();
    head.strip_suffix(':')
}

/// EVERY label in `ir` matching `<prefix><digits>`. A probe's function can be
/// emitted more than once (specialized method clones, inlined constructors),
/// and each copy carries its own gate — verification must hold for ALL of
/// them, or a sabotage that bypasses one copy hides behind the other (this
/// exact miss was caught by the S4 sabotage test on `idxset.recv_prop`).
fn all_numbered_labels(ir: &str, prefix: &str) -> Vec<String> {
    ir.lines()
        .filter_map(label_of)
        .filter(|l| label_matches(l, prefix))
        .map(str::to_string)
        .collect()
}

/// The `br i1` whose TRUE target is exactly `%<label>`, plus the body of the
/// block containing it (block start to the branch).
fn branch_into_exact(ir: &str, label: &str) -> Option<(String, String)> {
    let needle = format!("label %{label}");
    let mut current_body: Vec<&str> = Vec::new();
    for line in ir.lines() {
        if label_of(line).is_some() {
            current_body.clear();
            continue;
        }
        let t = line.trim();
        if t.starts_with("br i1 ") {
            if let Some(pos) = t.find(&needle) {
                let after = t.as_bytes().get(pos + needle.len());
                if matches!(after, None | Some(b',') | Some(b' ')) {
                    // TRUE target only: the text before the needle must not
                    // already contain another label operand.
                    if !t[..pos].contains("label %") {
                        return Some((t.to_string(), current_body.join("\n")));
                    }
                }
            }
        }
        current_body.push(line);
    }
    None
}

/// Body of the block whose label is exactly `label`.
fn block_body_exact(ir: &str, label: &str) -> Option<String> {
    let mut inside = false;
    let mut out: Vec<&str> = Vec::new();
    for line in ir.lines() {
        if let Some(l) = label_of(line) {
            if inside {
                break;
            }
            inside = l == label;
            continue;
        }
        if inside {
            out.push(line);
            let t = line.trim();
            if t.starts_with("br ") || t.starts_with("ret ") {
                break;
            }
        }
    }
    (inside && !out.is_empty()).then(|| out.join("\n"))
}

/// The condition token of a `br i1 <cond>, label ...`. `Err` for a constant —
/// a hard-wired branch is sabotage shape S2, never valid emitter output.
fn live_branch_condition(branch: &str) -> Result<String, String> {
    let cond = branch
        .trim_start()
        .strip_prefix("br i1 ")
        .and_then(|rest| rest.split(',').next())
        .map(str::trim)
        .ok_or_else(|| format!("cannot read a condition from `{branch}`"))?;
    if !cond.starts_with('%') {
        return Err(format!(
            "the gate is hard-wired to `{cond}` — the predicate is dead and the \
             barrier unconditionally taken or skipped: `{branch}`"
        ));
    }
    Ok(cond.to_string())
}

/// Def-chain walk: `cond` must be the OR of the `GC_FLAG_TENURED` header test
/// and the incremental-count test, with every link DEFINED in `body` — not
/// merely present somewhere near the branch.
fn check_generation_predicate(body: &str, cond: &str) -> Result<(), String> {
    let def = def_of(body, cond)
        .ok_or_else(|| format!("branch condition {cond} is not defined in its block"))?;
    if !def.starts_with("or i1") {
        return Err(format!(
            "generation gate must be `or i1 <tenured>, <incremental>`, got `{def}`"
        ));
    }
    let a = operand(def, 0).ok_or("or has no first operand")?;
    let b = operand(def, 1).ok_or("or has no second operand")?;
    let (tenured, incremental) = {
        let a_def = def_of(body, &a).unwrap_or_default();
        if a_def.starts_with("icmp ne i8") {
            (a, b)
        } else {
            (b, a)
        }
    };
    let tenured_def = def_of(body, &tenured)
        .ok_or_else(|| format!("tenured clause {tenured} is not defined in its block"))?;
    if !tenured_def.starts_with("icmp ne i8") {
        return Err(format!(
            "tenured clause must be `icmp ne i8`, got `{tenured_def}`"
        ));
    }
    let masked = operand(tenured_def, 0).ok_or("tenured icmp names no register")?;
    let masked_def = def_of(body, &masked)
        .ok_or_else(|| format!("tenured mask {masked} is not defined in its block"))?;
    if !(masked_def.starts_with("and i8") && masked_def.ends_with(", 32")) {
        return Err(format!(
            "tenured clause must mask GC_FLAG_TENURED (`and i8 …, 32`), got `{masked_def}`"
        ));
    }
    let flags = operand(masked_def, 0).ok_or("tenured mask names no register")?;
    let flags_def = def_of(body, &flags)
        .ok_or_else(|| format!("gc_flags load {flags} is not defined in its block"))?;
    if !flags_def.starts_with("load i8") {
        return Err(format!(
            "tenured clause must read the parent's header byte, got `{flags_def}`"
        ));
    }
    let incremental_def = def_of(body, &incremental)
        .ok_or_else(|| format!("incremental clause {incremental} is not defined in its block"))?;
    if !incremental_def.starts_with("icmp ne i32") {
        return Err(format!(
            "incremental clause must be `icmp ne i32`, got `{incremental_def}`"
        ));
    }
    let count = operand(incremental_def, 0).ok_or("incremental icmp names no register")?;
    let count_def = def_of(body, &count)
        .ok_or_else(|| format!("incremental count {count} is not defined in its block"))?;
    if !count_def.contains(INCREMENTAL_GLOBAL) {
        return Err(format!(
            "incremental clause must load {INCREMENTAL_GLOBAL}, got `{count_def}`"
        ));
    }
    Ok(())
}

/// Def-chain walk for `emit_may_carry_heap_pointer_check`:
/// `or(or-chain of tag compares, and(top16 == 0, bits >= floor))`.
fn check_value_predicate(body: &str, cond: &str) -> Result<(), String> {
    let def = def_of(body, cond)
        .ok_or_else(|| format!("value-gate condition {cond} is not defined in its block"))?;
    if !def.starts_with("or i1") {
        return Err(format!(
            "value gate must be the tag-OR-bare-address disjunction, got `{def}`"
        ));
    }
    let tagged = operand(def, 0).ok_or("value gate has no tagged operand")?;
    let bare = operand(def, 1).ok_or("value gate has no bare-address operand")?;
    if !def_of(body, &tagged).is_some_and(|d| d.starts_with("or i1")) {
        return Err(format!(
            "the tagged half of the value gate must be an OR of the pointer/string/\
             bigint tag compares, defined in the same block: {tagged}"
        ));
    }
    if !def_of(body, &bare).is_some_and(|d| d.starts_with("and i1")) {
        return Err(format!(
            "the bare-address half of the value gate must be `top16 == 0 AND bits >= \
             floor`, defined in the same block: {bare}"
        ));
    }
    Ok(())
}

/// One gate instance: `label` must be entered by a LIVE `cond_br` whose
/// predicate def-chain-checks with `check`, and (when `require_call`) the
/// block must still make the barrier call.
fn verify_gate_instance(
    ir: &str,
    stem: &str,
    label: &str,
    check: fn(&str, &str) -> Result<(), String>,
    require_call: bool,
) -> Result<String, String> {
    let (branch, pred_body) = branch_into_exact(ir, label).ok_or_else(|| {
        format!(
            "{stem}: no `br i1 … label %{label}` — this gate instance is dead IR or              bypassed (an emitted block is not a reached block)"
        )
    })?;
    let cond = live_branch_condition(&branch)?;
    check(&pred_body, &cond).map_err(|e| format!("{stem} gate into {label}: {e}"))?;
    let body =
        block_body_exact(ir, label).ok_or_else(|| format!("{stem}: block %{label} has no body"))?;
    if require_call && !body.contains(BARRIER_CALL) {
        return Err(format!(
            "{stem}: block %{label} no longer calls js_write_barrier_slot — the              barrier was deleted or moved out of its gated block:\n{body}"
        ));
    }
    Ok(pred_body)
}

/// The uniform floor, over EVERY instance of the stem's gates in `ir`.
/// `Err` is the red verdict; every message names what broke.
pub(super) fn verify_stem_ir(ir: &str, stem: &str, kind: StemKind) -> Result<(), String> {
    let barrier_labels = all_numbered_labels(ir, &format!("{stem}.barrier."));
    if barrier_labels.is_empty() {
        return Err(format!(
            "no `{stem}.barrier.<n>` block anywhere in the emitted IR — the barrier              arm was deleted, or the probe no longer reaches the tier"
        ));
    }
    for label in &barrier_labels {
        verify_gate_instance(ir, stem, label, check_generation_predicate, true)?;
    }
    match kind {
        StemKind::GenerationTested => {}
        StemKind::ValueAndGenerationTested => {
            let maybe = all_numbered_labels(ir, &format!("{stem}.barrier.maybe."));
            if maybe.len() != barrier_labels.len() {
                return Err(format!(
                    "{stem}: {} barrier instance(s) but {} value-gate instance(s) —                      a barrier lost its value gate or vice versa",
                    barrier_labels.len(),
                    maybe.len()
                ));
            }
            for label in &maybe {
                verify_gate_instance(ir, stem, label, check_value_predicate, false)?;
            }
        }
        StemKind::PointerTestedStore => {
            let book = all_numbered_labels(ir, &format!("{stem}.gc_bookkeeping."));
            if book.len() != barrier_labels.len() {
                return Err(format!(
                    "{stem}: {} barrier instance(s) but {} bookkeeping-gate                      instance(s) — a barrier lost its value gate or vice versa",
                    barrier_labels.len(),
                    book.len()
                ));
            }
            for label in &book {
                let pred_body =
                    verify_gate_instance(ir, stem, label, check_value_predicate, false)?;
                if !pred_body
                    .lines()
                    .any(|l| l.trim().starts_with("store double"))
                {
                    return Err(format!(
                        "{stem}: the slot store must stay UNCONDITIONAL, in the block                          that branches into %{label} — a guard that also skips the                          store is a dropped write:\n{pred_body}"
                    ));
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Probes. One per registry entry; `probe_ir` is an exhaustive match so a
// registry entry without a probe fails at test time with its own name.
// ---------------------------------------------------------------------------

const ARR_ID: u32 = 20;
const IDX_ID: u32 = 21;
const VAL_ID: u32 = 22;

/// `function probe(v) { let a = []; for (let i = 0; i < 8; i++) a[i] = v; }` —
/// a LOCAL array receiver is what routes the store through
/// `lower_index_set_fast` (it needs a slot to write a realloc'd head back to),
/// the integer-literal-seeded counter is what keeps the numeric index off the
/// runtime-key fallback, and `v: Any` is what puts the store on the live-test
/// tier at all (same fixture reasoning as `index_set_barrier_tests::setter`).
fn idxset_inbounds_ir() -> String {
    let mut m = Module::new("idxset_inbounds_census.ts");
    m.functions = vec![Function {
        id: 1,
        name: "probe".to_string(),
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
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: ARR_ID,
                name: "a".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Array(Vec::new())),
            },
            Stmt::For {
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
                    object: Box::new(Expr::LocalGet(ARR_ID)),
                    index: Box::new(Expr::LocalGet(IDX_ID)),
                    value: Box::new(Expr::LocalGet(VAL_ID)),
                })],
            },
            Stmt::Return(Some(Expr::LocalGet(ARR_ID))),
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
    String::from_utf8(compile_module(&m, ir_opts()).expect("module compiles"))
        .expect("LLVM IR should be UTF-8")
}

/// `const a = []; a.push({v: 1})` — the pointer-valued push whose barrier
/// #7511 gates (same fixture as `array_push.rs::parent_gate_tests`).
fn apush_ir() -> String {
    let mut m = Module::new("apush_census.ts");
    m.functions = vec![Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: ARR_ID,
                name: "a".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Array(Vec::new())),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: ARR_ID,
                value: Box::new(Expr::Object(vec![("v".to_string(), Expr::Number(1.0))])),
                field_writeback: None,
            }),
            Stmt::Return(Some(Expr::LocalGet(ARR_ID))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    m.init_kind = ModuleInitKind::Eager;
    String::from_utf8(compile_module(&m, ir_opts()).expect("module compiles"))
        .expect("LLVM IR should be UTF-8")
}

/// `class Boxed { v: any; constructor(v) { this.v = v } }` plus an escaping
/// `new Boxed(1)` — the complete parameter-to-field constructor is what selects
/// constructor-free prologue stores, and the boxed field requires their
/// value-and-generation-tested barrier.
fn ctor_prologue_ir() -> String {
    super::class_field_barrier_tests::ir()
}

/// The pristine IR for a stem. Exhaustive on the registry: an entry added
/// without a probe panics HERE, named, instead of passing silently.
fn probe_ir(stem: &str) -> String {
    match stem {
        "apush" => apush_ir(),
        "class_field_set" => super::class_field_barrier_tests::ir(),
        "ctor_prologue" => ctor_prologue_ir(),
        "idxset.inbounds" => idxset_inbounds_ir(),
        "idxset.recv_prop" => super::index_set_barrier_tests::ir(),
        "put.pic" => super::write_pic_barrier_tests::census_put_pic_ir(),
        other => panic!(
            "VERIFIED_BARRIER_STEMS entry {other:?} has no probe in \
             barrier_stem_census_tests::probe_ir — a registry entry without a \
             probe is a claim without evidence"
        ),
    }
}

// ---------------------------------------------------------------------------
// The census, and the sabotage matrix that proves it can fail.
// ---------------------------------------------------------------------------

#[test]
fn census_every_registered_stem_has_a_live_verified_witness() {
    assert_default_barrier_env_not_disabled();
    for &(stem, kind) in VERIFIED_BARRIER_STEMS {
        let ir = probe_ir(stem);
        if let Err(err) = verify_stem_ir(&ir, stem, kind) {
            panic!("stem {stem:?} failed its IR witness: {err}");
        }
    }
}

/// Surgery helpers assert they CHANGED the IR, so a sabotage that misses its
/// target fails as a broken test instead of passing as a vacuous one.
fn assert_changed(original: &str, doctored: &str, what: &str) -> String {
    assert_ne!(original, doctored, "sabotage `{what}` did not alter the IR");
    doctored.to_string()
}

/// S1 — delete only the barrier: every `js_write_barrier_slot` CALL line goes;
/// blocks, guards and predicates all stay.
#[test]
fn sabotage_deleting_the_barrier_call_goes_red_for_every_stem() {
    assert_default_barrier_env_not_disabled();
    for &(stem, kind) in VERIFIED_BARRIER_STEMS {
        let ir = probe_ir(stem);
        verify_stem_ir(&ir, stem, kind).expect("pristine IR must verify first");
        let doctored: String = ir
            .lines()
            .filter(|l| !l.contains(BARRIER_CALL))
            .collect::<Vec<_>>()
            .join("\n");
        let doctored = assert_changed(&ir, &doctored, "delete barrier call");
        assert!(
            verify_stem_ir(&doctored, stem, kind).is_err(),
            "stem {stem:?}: deleting the barrier call must be caught"
        );
    }
}

/// S2 — `br i1 true` with the predicate left DEAD in the block. Textually the
/// barrier and its whole predicate are still present; only the def-chain walk
/// can tell the branch stopped consulting it.
#[test]
fn sabotage_hardwiring_the_gate_goes_red_for_every_stem() {
    assert_default_barrier_env_not_disabled();
    for &(stem, kind) in VERIFIED_BARRIER_STEMS {
        let ir = probe_ir(stem);
        verify_stem_ir(&ir, stem, kind).expect("pristine IR must verify first");
        let label = all_numbered_labels(&ir, &format!("{stem}.barrier."))
            .into_iter()
            .next()
            .expect("a gated barrier block exists");
        let (branch, _) = branch_into_exact(&ir, &label).expect("gated branch exists");
        let cond = live_branch_condition(&branch).expect("pristine branch is live");
        let hardwired = branch.replacen(&cond, "true", 1);
        let doctored = ir.replacen(&branch, &hardwired, 1);
        let doctored = assert_changed(&ir, &doctored, "hardwire branch true");
        assert!(
            verify_stem_ir(&doctored, stem, kind).is_err(),
            "stem {stem:?}: `br i1 true` with a dead predicate must be caught"
        );
    }
}

/// S3 — the barrier moved to another block: the gated block keeps its label
/// and its branch, but the call now lives in a block the stem does not name.
/// (This is also what a marker left behind after a refactor looks like.)
#[test]
fn sabotage_moving_the_barrier_out_of_its_block_goes_red_for_every_stem() {
    assert_default_barrier_env_not_disabled();
    for &(stem, kind) in VERIFIED_BARRIER_STEMS {
        let ir = probe_ir(stem);
        verify_stem_ir(&ir, stem, kind).expect("pristine IR must verify first");
        let first_label = all_numbered_labels(&ir, &format!("{stem}.barrier."))
            .into_iter()
            .next()
            .expect("a barrier block exists");
        let barrier_block = block_body_exact(&ir, &first_label).expect("barrier block has a body");
        let call_line = barrier_block
            .lines()
            .find(|l| l.contains(BARRIER_CALL))
            .expect("barrier block contains the call");
        // Move the call into the entry block (any other block will do): remove
        // it from its gated block, re-emit it at the top of the function body.
        let removed: String = ir
            .lines()
            .filter(|l| *l != call_line)
            .collect::<Vec<_>>()
            .join("\n");
        let mut moved_lines: Vec<String> = Vec::new();
        let mut inserted = false;
        for line in removed.lines() {
            moved_lines.push(line.to_string());
            if !inserted && !line.starts_with(char::is_whitespace) && line.trim_end().ends_with(':')
            {
                moved_lines.push(call_line.to_string());
                inserted = true;
            }
        }
        let doctored = assert_changed(&ir, &moved_lines.join("\n"), "move barrier call");
        assert!(
            verify_stem_ir(&doctored, stem, kind).is_err(),
            "stem {stem:?}: a barrier moved out of its gated block must be caught"
        );
    }
}

/// S4 — the arm goes DEAD: the gated branch becomes an unconditional jump to
/// its false edge. The barrier block, its label and its call all survive as
/// unreachable IR — the shape #8183's third sabotage initially slipped past a
/// content-only check with.
#[test]
fn sabotage_bypassing_the_gate_goes_red_for_every_stem() {
    assert_default_barrier_env_not_disabled();
    for &(stem, kind) in VERIFIED_BARRIER_STEMS {
        let ir = probe_ir(stem);
        verify_stem_ir(&ir, stem, kind).expect("pristine IR must verify first");
        let label = all_numbered_labels(&ir, &format!("{stem}.barrier."))
            .into_iter()
            .next()
            .expect("a gated barrier block exists");
        let (branch, _) = branch_into_exact(&ir, &label).expect("gated branch exists");
        let false_target = operand(&branch, 2).expect("branch has a false target");
        let bypass = format!("br label {false_target}");
        let doctored = ir.replacen(branch.trim_start(), &bypass, 1);
        let doctored = assert_changed(&ir, &doctored, "bypass gate");
        assert!(
            verify_stem_ir(&doctored, stem, kind).is_err(),
            "stem {stem:?}: an unconditionally-bypassed gate must be caught"
        );
    }
}
