//! Charter step 4b (#10884): LOOP regions and per-iteration (loop-body)
//! regions. Design: `/root/linktime/DESIGN.md` §3.0–§3.2 (approved
//! 2026-09-28).
//!
//! # What this does
//!
//! A loop whose body reads or writes static keys of a receiver that cannot
//! change inside the loop (`this`, or a parameter / local no closure mutates
//! and the loop never assigns) is entered through ONE guard per receiver, in
//! the preheader:
//!
//! ```text
//! preheader:  receiver test + ONE ShapeId compare against the region word
//!             (+ the two per-object store facts, when the region stores)
//!             -> valid (an i1 slot) and the region word's slot vector
//! body top:   !valid                 -> G-body (today's lowering)
//!             [body can re-enter JS] valid := ([p+4] == S) && no-proof
//!             F-body                  (facts: bare loads / stores)
//! ```
//!
//! Only the BODY is lowered twice; the loop's own mechanics (counter
//! representation, condition, update, polls) are lowered once by whichever
//! tier takes the loop, so no tier needs a hand-off protocol (DESIGN §3.0).
//! F-body and G-body rejoin at the latch. That is sound because a fact is
//! used only inside F-body, and F-body is entered only when `valid` was
//! established by the guard or the re-check with no JS in between: the
//! re-check sits at the TOP of every iteration.
//!
//! A per-iteration receiver (`const o = objs[k & 7]; o.d = k; ...`) gets the
//! same split for the TAIL of the body after its binding, with the full guard
//! at the tail's top every iteration (no `valid` slot).
//!
//! # Which accesses are bare
//!
//! [`plan`] is the planner and the census in one pass (DESIGN §3.4): it walks
//! the body in JS evaluation order with the facts FRESH at the split point, and
//! anything that can run JS — a call, `new`, a coercion of an unproven operand,
//! another receiver's access (a getter), an index access, a non-bare store —
//! makes them STALE for the rest of the iteration. An access reached FRESH is
//! bare; every other access is today's tower. After lowering, [`verify`]
//! recomputes the same fact state over the EMITTED blocks from the calls that
//! were actually emitted; a bare access reachable after a call that may run JS
//! discards F-body (the split then branches to G-body unconditionally). A plan
//! that is wrong can cost speed, never a value.
//!
//! # GC
//!
//! No pointer is held across anything: every bare access re-derives the
//! receiver handle from the binding's root (`load; bitcast; and POINTER_MASK`
//! — a derivation `root_reload.rs` and the gc-root-dominance checker both
//! track), and LLVM merges re-derivations inside collection-free stretches. S
//! and the slots are plain integers.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use perry_hir::{BinaryOp, CompareOp, Expr, Stmt, UnaryOp};

use crate::expr::receiver_range::Route;
use crate::expr::region_guard::{self, Sites, MAX_KEYS};
use crate::expr::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I1, I16, I32, I64, I8};

mod arrays;
mod bare;
mod guard;
mod plan;
mod verify;

use self::arrays::ArrayRecv;
pub(crate) use self::arrays::{
    alias_clone, emit_poll_refresh, is_bare_index_get, try_lower_bare_index_get, unalias_clone,
};
use self::bare::note;
pub(crate) use self::bare::{try_lower_bare_get, try_lower_bare_put, try_lower_fact_add_tree};
use self::guard::{
    decode_slots, emit_body_guard_direct, emit_guard, emit_guard_word, field_i32, handle_of,
    lower_recv, store_admission,
};
use self::plan::{
    accesses, assigned, body_nodes, body_refused, fact_tree_leaves, plan, receiver_eligible, Plan,
    Recheck,
};
use self::verify::{successors, verify};

const SLOT_BITS: u32 = 6;
const PRIME_ATTEMPTS: &str = "8";
/// `REGION_GUARD_WORD_EMPTY`.
const EMPTY_WORD: &str = "4294967295";
/// `REGION_LOOP_WORD_RETIRED` (all ones): the runtime publishes it when the
/// last bounded prime attempt fails, and the guard then skips even the
/// receiver test (DESIGN §4.3: "the retirement check moves into the word").
const RETIRED_WORD: &str = "-1";
/// `PACKED_SPILL_FLIP` as an `i32` operand: a loop word naming a SPILL-located
/// key carries its ShapeId with these bits flipped (the S5 convention), so the
/// guard's plain compare admits only all-inline words and a second compare, on
/// the miss side, recognises a spill word.
const FLIP_I32: &str = "-1073741824";
/// `OBJ_FLAG_PLAIN_ORDINARY | OBJ_FLAG_TYPED_ARRAY_PROTO` and its admitted
/// value — DESIGN §6.5a fact F-A (class-less receivers).
const CLASSLESS_ADMIT_MASK_I16: &str = "768";
const CLASSLESS_ADMIT_I16: &str = "512";

/// `PERRY_REGIONS=0` switches loop regions off in ONE compiler (A/B arm);
/// `PERRY_REGION_READS=0` (slices 1/2) switches them off too.
/// Does a region pay for the code it copies? A loop region versions the whole
/// loop and a body region copies its tail, so the HIR copied per bare access
/// is bounded by [`NODES_PER_BARE`] (`PERRY_REGION_NODES_PER_BARE` overrides
/// it for measurement). `PERRY_REGION_DIAG=3` prints every candidate's size.
///
/// Measured 2026-09-28: every matrix candidate is at most 13 nodes per bare
/// access (406 candidates); tsc + Zod offer 128 candidates at a median of 28
/// and up to 201, 6,065 nodes in all, for 23 region entries in a whole
/// transpile. At 16, all matrix regions stay and tsc/Zod keep 39 regions
/// (649 nodes, 63 bare accesses): the copies that bought nothing go.
const NODES_PER_BARE: usize = 16;

fn pays(ctx: &FnCtx<'_>, kind: &str, nodes: usize, bare: usize) -> bool {
    let limit: usize = std::env::var("PERRY_REGION_NODES_PER_BARE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(NODES_PER_BARE);
    if std::env::var("PERRY_REGION_DIAG").as_deref() == Ok("3") {
        eprintln!(
            "[perry region] candidate kind={kind} nodes={nodes} bare={bare} fn={}",
            ctx.func.name
        );
    }
    nodes <= limit.saturating_mul(bare.max(1))
}

/// `PERRY_REGION_SPILL=0` (compile time): no spill-reading copies. The
/// runtime is then told every key is stored, which is exactly the condition
/// under which it never publishes a spill word, so no copy is needed.
fn effective_stored_mask(stored: u32, keys: usize) -> u32 {
    let off = matches!(
        std::env::var("PERRY_REGION_SPILL").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    );
    if off {
        (1u32 << keys) - 1
    } else {
        stored
    }
}

fn disabled() -> bool {
    matches!(
        std::env::var("PERRY_REGIONS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    ) || region_guard::disabled()
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub(crate) enum Recv {
    Local(u32),
    This,
}

impl Recv {
    fn of(e: &Expr) -> Option<Recv> {
        match e {
            Expr::LocalGet(id) => Some(Recv::Local(*id)),
            Expr::This => Some(Recv::This),
            _ => None,
        }
    }
    fn expr(self) -> Expr {
        match self {
            Recv::Local(id) => Expr::LocalGet(id),
            Recv::This => Expr::This,
        }
    }
}

/// One receiver of an admitted region, as lowering needs it.
#[derive(Clone)]
pub(crate) struct Receiver {
    pub(crate) recv: Recv,
    pub(crate) keys: Vec<String>,
    pub(crate) has_store: bool,
    /// Bit `i`: the body stores `keys[i]` (the runtime then requires it inline).
    stored_mask: u32,
    /// Bit `i`: a bare store may write `keys[i]` a value not proven a
    /// canonical double (the word must then give it an `Any` lane).
    boxed_mask: u32,
    /// Bit `i`: a read assumes the key has an identity F64 lane.
    r_mask: u32,
    /// `i1`: the guard matched this receiver's SPILL word (flipped id).
    spill: String,
    sites: Option<(String, String)>,
    /// The region word, an SSA value of the preheader (loop regions) or of
    /// the tail's guard block (body regions).
    word: String,
    /// Each key's slot (`i64`), decoded once from `word`.
    slots: Vec<String>,
}

/// Whether this exact fresh bare read is protected by an R bit in an
/// active F clone. The published word (or exact static birth id) guarantees
/// the slot holds a canonical raw JS Number; G and post-loop have no Active
/// fact and therefore never answer true.
pub(crate) fn is_f64_read(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    let Expr::PropertyGet {
        object, property, ..
    } = e
    else {
        return false;
    };
    let Some(recv) = Recv::of(object) else {
        return false;
    };
    let ptr = e as *const Expr as usize;
    // bare::try_lower_bare_get consults only the innermost Active fact.
    // Looking through the stack would claim Number for a read the inner
    // clone actually lowers through the generic path.
    ctx.region_loop_facts.last().is_some_and(|facts| {
        facts.bare.contains(&ptr)
            && facts.receivers.iter().any(|rv| {
                rv.recv == recv
                    && rv
                        .keys
                        .iter()
                        .position(|key| key == property)
                        .is_some_and(|i| rv.r_mask & (1 << i) != 0)
            })
    })
}

/// A region whose body has not been lowered yet: `lower_stmts` recognises the
/// body slice by address and splits it.
#[derive(Clone)]
pub(crate) struct Pending {
    body_ptr: usize,
    body_len: usize,
    /// First statement of the split tail (0 for a loop region).
    split_at: usize,
    /// Loop region: the i1 slot the preheader guard and the re-check write.
    valid_slot: Option<String>,
    /// Loop region: set by a fact tree's generic arm.
    dirty_slot: Option<String>,
    recheck: Recheck,
    receivers: Vec<Receiver>,
    bare: HashSet<usize>,
    bare_reads: Vec<(usize, Recv, String)>,
    number_local_uses: HashSet<u32>,
    declared_locals: HashSet<u32>,
    number_locals: Vec<u32>,
    entry_tests: Vec<u32>,
    trees: HashSet<usize>,
    token: u64,
    /// Loop regions: which split copy [`lower_loop`] is lowering — the one
    /// for a word with a spill-located key, or the all-inline one.
    spill_mode: bool,
    /// Loop regions: array receivers (S3), guarded in the preheader.
    arrays: Vec<ArrayRecv>,
    /// Loop regions with array receivers: the body region split inside
    /// F-body (a per-iteration receiver read from the array). Its fact
    /// trees' generic arms set the loop's dirty flag (its `dirty_slot` is the
    /// loop's); its G-tail leaves the loop region (`parent_valid`).
    inner: Option<Box<Pending>>,
    /// A body region nested in a loop region: the loop's valid slot. A
    /// failed body guard means the per-iteration receiver is not what the
    /// region serves; re-deriving the loop's facts after every such
    /// iteration costs more than the straight-line read it replaces, so the
    /// G-tail clears the loop's valid flag and the loop runs G-body (the
    /// judgement that refuses a plan which re-checks every iteration, made
    /// where the failure is seen).
    parent_valid: Option<String>,
    /// The copy of that body region inside the loop's G-body: the loop's
    /// dirty slot. A passing body guard there asks the next iteration to
    /// re-check the loop's facts (a receiver that failed while its layout
    /// was warming up comes back to F-body).
    retry: Option<String>,
}

/// The facts active while F-body is lowered.
pub(crate) struct Active {
    receivers: Vec<Receiver>,
    bare: HashSet<usize>,
    trees: HashSet<usize>,
    dirty_slot: Option<String>,
    /// Emitted bare accesses: (block index, instruction index) — what
    /// [`verify`] checks.
    emitted: Vec<(usize, usize)>,
    /// The last handle derived per receiver in this F-body, with where it was
    /// derived (block, instruction index) — [`region_handle`] reuses it while
    /// nothing that can collect lies between.
    handles: Vec<(Recv, String, usize, usize)>,
    /// This F copy serves words with a SPILL-located key (bit 62): a read's
    /// slot field then says inline (`< 32`) or spill index (`32 + i`).
    spill: bool,
    /// Array receivers (S3) and the emitted bare element reads.
    arrays: Vec<ArrayRecv>,
    emitted_arr: Vec<(usize, usize)>,
}

thread_local! {
    /// Module-level bindings declared `const` in the module being compiled:
    /// invariant after initialisation, so a region may guard them once.
    static CONST_MODULE_GLOBALS: std::cell::RefCell<HashSet<u32>> =
        std::cell::RefCell::new(HashSet::new());
    static NEXT_TOKEN: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    static STATS: std::cell::RefCell<[u64; 6]> = const { std::cell::RefCell::new([0; 6]) };
}

/// `PERRY_REGION_DIAG=1` counters: loop regions formed, body regions formed,
/// bare reads, bare stores, F-bodies discarded by the verifier, loops refused.
fn stat(i: usize, n: u64) {
    STATS.with(|s| s.borrow_mut()[i] += n);
}

/// Per-module setup, from codegen's module entry.
pub(crate) fn begin_module(hir: &perry_hir::Module) {
    let set: HashSet<u32> = hir
        .init
        .iter()
        .filter_map(|s| match s {
            Stmt::Let {
                id, mutable: false, ..
            } => Some(*id),
            _ => None,
        })
        .collect();
    CONST_MODULE_GLOBALS.with(|c| *c.borrow_mut() = set);
}

fn const_module_global(id: u32) -> bool {
    CONST_MODULE_GLOBALS.with(|c| c.borrow().contains(&id))
}

pub(crate) fn take_stats() -> [u64; 6] {
    STATS.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

fn candidates_for_loop(
    ctx: &FnCtx<'_>,
    cond: Option<&Expr>,
    body: &[Stmt],
    update: Option<&Expr>,
) -> HashSet<Recv> {
    let mut extra: Vec<&Expr> = Vec::new();
    if let Some(c) = cond {
        extra.push(c);
    }
    if let Some(u) = update {
        extra.push(u);
    }
    let written = assigned(body, &extra);
    accesses(body)
        .into_iter()
        .map(|(r, _, _)| r)
        .filter(|r| match r {
            Recv::Local(id) => !written.contains(id),
            Recv::This => true,
        })
        .filter(|r| receiver_eligible(ctx, *r))
        .collect()
}

/// Called by the loop lowerings right before the loop is lowered (after a
/// `for`'s init, after the specialised tiers declined). Plans the region and,
/// when admitted, emits the preheader guard and registers the body for the
/// split. Returns a token for [`end`].
pub(crate) fn begin(
    ctx: &mut FnCtx<'_>,
    cond: Option<&Expr>,
    body: &[Stmt],
    update: Option<&Expr>,
) -> Result<Option<u64>> {
    begin_with(ctx, cond, body, update, false)
}

/// [`begin`] for a loop a specialised tier versioned and whose slow copy
/// still reads arrays: only a region with a bare element read is formed.
pub(crate) fn begin_for_arrays(
    ctx: &mut FnCtx<'_>,
    cond: Option<&Expr>,
    body: &[Stmt],
    update: Option<&Expr>,
) -> Result<Option<u64>> {
    begin_with(ctx, cond, body, update, true)
}

fn begin_with(
    ctx: &mut FnCtx<'_>,
    cond: Option<&Expr>,
    body: &[Stmt],
    update: Option<&Expr>,
    arrays_only: bool,
) -> Result<Option<u64>> {
    if disabled()
        || crate::codegen::full_outline_ic_enabled()
        || crate::expr::typed_feedback_emission_enabled()
        || !ctx.pending_labels.is_empty()
        || in_call_free_clone(ctx)
        || !ctx.region_loop_facts.is_empty()
        || body_refused(body)
        || cond.is_some_and(expr_refused)
        || update.is_some_and(expr_refused)
    {
        return Ok(None);
    }
    // A loop region; failing that, a body region (per-iteration receiver).
    let cands = candidates_for_loop(ctx, cond, body, update);
    let arrs = arrays::candidates(ctx, cond, body, update);
    // Array receivers (S3): the loop region, with the body region nested in
    // its F-body when there is one. Without a bare element read, today's
    // choice below.
    let mut nested: Option<(Plan, Option<(usize, Plan)>)> = None;
    if !arrs.is_empty() {
        let inner = body_region_plan(ctx, body);
        let p = plan(
            ctx,
            body,
            cands.clone(),
            arrs,
            Some((cond, update)),
            inner.as_ref().map(|(k, ip)| (*k, &ip.bare, &ip.trees)),
        )
        // A re-check every iteration re-derives the array's facts every
        // iteration: that is the straight-line read's cost, plus a split.
        .filter(|p| !p.arrays.is_empty() && p.recheck != Recheck::Always);
        if std::env::var("PERRY_REGION_DIAG").as_deref() == Ok("4") {
            eprintln!(
                "[perry region] array plan in {}: {:?}",
                ctx.func.name,
                p.as_ref()
                    .map(|p| (p.recheck, p.arrays.len(), p.bare.len()))
            );
        }
        if let Some(p) = p {
            let bare = p.bare.len() + inner.as_ref().map_or(0, |(_, ip)| ip.bare.len());
            if pays(ctx, "loop", body_nodes(body), bare) {
                nested = Some((p, inner));
            }
        }
    }
    if nested.is_none() && arrays_only {
        return Ok(None);
    }
    let (first, inner) = match nested {
        Some((p, inner)) => (Some(p), inner),
        None => (
            plan(ctx, body, cands, HashMap::new(), Some((cond, update)), None)
                .filter(|p| pays(ctx, "loop", body_nodes(body), p.bare.len())),
            None,
        ),
    };
    if let Some(p) = first {
        let token = NEXT_TOKEN.with(|t| {
            let v = t.get();
            t.set(v + 1);
            v
        });
        let valid_slot = ctx.func.alloca_entry(I1);
        let mut receivers: Vec<Receiver> = p
            .receivers
            .iter()
            .map(|(r, k, st, sm, bm, rm)| Receiver {
                recv: *r,
                keys: k.clone(),
                has_store: *st,
                stored_mask: effective_stored_mask(*sm, k.len()),
                boxed_mask: *bm,
                r_mask: *rm,
                spill: "false".to_string(),
                sites: None,
                word: String::new(),
                slots: Vec::new(),
            })
            .collect();
        let mut all = "true".to_string();
        for rv in receivers.iter_mut() {
            let (word, pass, spill) = emit_guard(ctx, rv)?;
            rv.spill = spill;
            all = ctx.block().and(I1, &all, &pass);
            decode_slots(ctx, rv, &word);
        }
        let mut arrs: Vec<ArrayRecv> = Vec::new();
        for (r, m) in &p.arrays {
            let a = ArrayRecv {
                recv: *r,
                max_index: *m,
                base_slot: ctx.func.alloca_entry(I64),
            };
            let pass = arrays::emit_guard(ctx, &a)?;
            all = ctx.block().and(I1, &all, &pass);
            arrs.push(a);
        }
        let (number_locals, entry_tests) = number_facts(
            ctx,
            body,
            &receivers,
            &p.bare_reads,
            &p.number_local_uses,
            &p.declared_locals,
        );
        if !entry_tests.is_empty() {
            let number_ok = emit_number_entry_tests(ctx, &entry_tests)?;
            all = ctx.block().and(I1, &all, &number_ok);
        }
        ctx.block().store(I1, &all, &valid_slot);
        let dirty_slot = ctx.func.alloca_entry(I1);
        ctx.block().store(I1, "false", &dirty_slot);
        stat(0, 1);
        let inner = inner.map(|(k, ip)| {
            let mut ib = body_pending(ip, body, k);
            ib.dirty_slot = Some(dirty_slot.clone());
            ib.parent_valid = Some(valid_slot.clone());
            Box::new(ib)
        });
        ctx.region_loops.push(Pending {
            body_ptr: body.as_ptr() as usize,
            body_len: body.len(),
            split_at: 0,
            valid_slot: Some(valid_slot),
            dirty_slot: Some(dirty_slot),
            recheck: p.recheck,
            receivers,
            bare: p.bare,
            bare_reads: p.bare_reads,
            number_local_uses: p.number_local_uses,
            declared_locals: p.declared_locals,
            number_locals,
            entry_tests,
            trees: p.trees,
            token,
            spill_mode: false,
            arrays: arrs,
            inner,
            parent_valid: None,
            retry: None,
        });
        return Ok(Some(token));
    }
    // Body region: the first `const o = <expr>` whose binding the rest of the
    // body reads or writes by static key.
    if let Some((k, p)) = body_region_plan(ctx, body) {
        let pending = body_pending(p, body, k);
        let token = pending.token;
        ctx.region_loops.push(pending);
        return Ok(Some(token));
    }
    stat(5, 1);
    Ok(None)
}

/// The body region of `body`: the first `const o = <expr>` whose binding the
/// rest of the body reads or writes by static key, and its plan.
fn body_region_plan(ctx: &FnCtx<'_>, body: &[Stmt]) -> Option<(usize, Plan)> {
    for (i, s) in body.iter().enumerate() {
        let Stmt::Let {
            id, init: Some(_), ..
        } = s
        else {
            continue;
        };
        let tail = &body[i + 1..];
        // Declared once, never assigned in the tail: a per-iteration constant.
        let decls = body
            .iter()
            .filter(|s| matches!(s, Stmt::Let { id: x, .. } if x == id))
            .count();
        if decls != 1 || assigned(tail, &[]).contains(id) || body_refused(tail) {
            continue;
        }
        // Eligibility of a body-declared binding is re-checked at the split,
        // after its declaration has been lowered.
        if ctx.boxed_vars.contains(id)
            || ctx.prealloc_boxes.contains(id)
            || ctx.tdz_boxes.contains(id)
            || ctx.local_slot_reps.contains_key(id)
            || ctx.integer_locals.contains(id)
        {
            continue;
        }
        let mut cands = HashSet::new();
        cands.insert(Recv::Local(*id));
        if let Some(p) = plan(ctx, tail, cands, HashMap::new(), None, None)
            .filter(|p| pays(ctx, "body", body_nodes(tail), p.bare.len()))
        {
            return Some((i + 1, p));
        }
    }
    None
}

/// Resolve the planner's exact fresh reads against the R guaranteed by
/// each receiver's chosen supplier, then run the existing Number-local
/// greatest fixed point with those reads as additional leaves.
fn number_facts(
    ctx: &FnCtx<'_>,
    tail: &[Stmt],
    receivers: &[Receiver],
    bare_reads: &[(usize, Recv, String)],
    number_local_uses: &HashSet<u32>,
    declared_locals: &HashSet<u32>,
) -> (Vec<u32>, Vec<u32>) {
    let f64_reads: HashSet<usize> = bare_reads
        .iter()
        .filter_map(|(ptr, recv, key)| {
            receivers.iter().find(|rv| rv.recv == *recv).and_then(|rv| {
                rv.keys
                    .iter()
                    .position(|k| k == key)
                    .filter(|i| rv.r_mask & (1 << i) != 0)
                    .map(|_| *ptr)
            })
        })
        .collect();
    if f64_reads.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let entry_candidates: HashSet<u32> = number_local_uses
        .iter()
        .copied()
        .filter(|id| {
            ctx.locals.contains_key(id)
                && !declared_locals.contains(id)
                && !ctx.boxed_vars.contains(id)
                && !ctx.module_globals.contains_key(id)
                && !ctx.number_by_construction_locals.contains(id)
        })
        .collect();
    let empty_inits = HashMap::new();
    let empty_ids = HashSet::new();
    let empty_fields = HashSet::new();
    let assumptions = crate::collectors::RegionNumberAssumptions {
        entry_candidates: &entry_candidates,
        static_numbers: ctx.number_by_construction_locals,
        f64_reads: &f64_reads,
    };
    let numeric = crate::collectors::collect_numeric_by_construction_locals_in_region(
        tail,
        &ctx.boxed_vars,
        ctx.module_globals,
        ctx.not_bigint_locals,
        &empty_inits,
        &empty_ids,
        &empty_ids,
        &empty_fields,
        Some(&assumptions),
    );
    let mut locals: Vec<u32> = numeric
        .iter()
        .copied()
        .filter(|id| !ctx.number_by_construction_locals.contains(id))
        .collect();
    locals.sort_unstable();
    let mut tests: Vec<u32> = locals
        .iter()
        .copied()
        .filter(|id| entry_candidates.contains(id))
        .collect();
    tests.sort_unstable();
    (locals, tests)
}

/// Strict Number entry condition for each loop-carried local that the F
/// clone assumes. The same check is repeated when G can re-enter F.
fn emit_number_entry_tests(ctx: &mut FnCtx<'_>, ids: &[u32]) -> Result<String> {
    let mut ok = "true".to_string();
    for &id in ids {
        let value = lower_expr(ctx, &Expr::LocalGet(id))?;
        let number = crate::stmt::loops::emit_js_value_is_number(ctx, &value);
        ok = ctx.block().and(I1, &ok, &number);
    }
    Ok(ok)
}

/// A body region's pending split at `split_at`.
fn body_pending(p: Plan, body: &[Stmt], split_at: usize) -> Pending {
    let token = NEXT_TOKEN.with(|t| {
        let v = t.get();
        t.set(v + 1);
        v
    });
    let receivers: Vec<Receiver> = p
        .receivers
        .iter()
        .map(|(r, k, st, sm, bm, rm)| Receiver {
            recv: *r,
            keys: k.clone(),
            has_store: *st,
            stored_mask: effective_stored_mask(*sm, k.len()),
            boxed_mask: *bm,
            r_mask: *rm,
            spill: "false".to_string(),
            sites: None,
            word: String::new(),
            slots: Vec::new(),
        })
        .collect();
    Pending {
        body_ptr: body.as_ptr() as usize,
        body_len: body.len(),
        split_at,
        valid_slot: None,
        dirty_slot: None,
        recheck: Recheck::None,
        receivers,
        bare: p.bare,
        bare_reads: p.bare_reads,
        number_local_uses: p.number_local_uses,
        declared_locals: p.declared_locals,
        number_locals: Vec::new(),
        entry_tests: Vec::new(),
        trees: p.trees,
        token,
        spill_mode: false,
        arrays: Vec::new(),
        inner: None,
        parent_valid: None,
        retry: None,
    }
}

fn expr_refused(e: &Expr) -> bool {
    body_refused(&[Stmt::Expr(e.clone())])
}

/// Lower the loop (`lower` is the tier dispatch that would run without a
/// region). A LOOP region is versioned at its preheader: when the guard
/// passed, the loop whose body is split (F-body / G-body, re-check at the top
/// of each iteration); when it did not, the loop exactly as it lowers without
/// a region. The choice is made once, before the first iteration, so neither
/// version ever hands the loop to the other — which is what keeps every tier's
/// own loop state (a private i32 counter, a hoisted bound) sound — and a
/// receiver the guard refuses pays one branch per loop ENTRY, not a split
/// body per iteration. A body region (per-iteration receiver) is not
/// versioned: its guard is inside the body.
pub(crate) fn lower_loop(
    ctx: &mut FnCtx<'_>,
    token: Option<u64>,
    lower: &mut dyn FnMut(&mut FnCtx<'_>) -> Result<()>,
) -> Result<()> {
    let Some(t) = token else {
        return lower(ctx);
    };
    let Some(pos) = ctx.region_loops.iter().position(|p| p.token == t) else {
        return lower(ctx);
    };
    let Some(valid_slot) = ctx.region_loops[pos].valid_slot.clone() else {
        return lower(ctx);
    };
    let split = ctx.new_block("rloop.version.split");
    let plain = ctx.new_block("rloop.version.plain");
    let merge = ctx.new_block("rloop.version.merge");
    let split_l = ctx.block_label(split);
    let plain_l = ctx.block_label(plain);
    let merge_l = ctx.block_label(merge);
    let v = ctx.block().load(I1, &valid_slot);
    ctx.block().cond_br(&v, &split_l, &plain_l);

    ctx.current_block = split;
    note(ctx, Route::RloopSplit);
    // A word naming a spill-located key selects the split copy that reads
    // through the spill buffer; every other word, the all-inline copy (the
    // hot one, whose reads are one load). A region that stores every key it
    // names never gets a spill word, so it needs no spill copy.
    let flags: Vec<String> = ctx.region_loops[pos]
        .receivers
        .iter()
        .map(|r| r.spill.clone())
        .collect();
    let may_spill = ctx.region_loops[pos]
        .receivers
        .iter()
        .any(|r| r.stored_mask != (1u32 << r.keys.len()) - 1);
    if may_spill {
        let inline_b = ctx.new_block("rloop.version.inline");
        let spill_b = ctx.new_block("rloop.version.spill");
        let inline_l = ctx.block_label(inline_b);
        let spill_l = ctx.block_label(spill_b);
        let any = any_flag(ctx, &flags);
        ctx.block().cond_br(&any, &spill_l, &inline_l);
        for (blk, mode) in [(inline_b, false), (spill_b, true)] {
            ctx.current_block = blk;
            if let Some(p) = ctx.region_loops.iter_mut().find(|p| p.token == t) {
                p.spill_mode = mode;
            }
            lower(ctx)?;
            if !ctx.block().is_terminated() {
                ctx.block().br(&merge_l);
            }
        }
    } else {
        lower(ctx)?;
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_l);
        }
    }

    // The plain version: the region is not registered while it lowers, so
    // its body is today's body. It is lowered LAST: a body's `Let`s are
    // declared by the first copy lowered, and a later copy re-declares them
    // through the reuse path (`let_stmt`, #1803), which does not refine the
    // binding's type from its initialiser — the plain loop can afford that,
    // the split loop's F-body cannot (read4_stmt/param: 95 -> 131 with the
    // plain copy first).
    let pos = ctx
        .region_loops
        .iter()
        .position(|p| p.token == t)
        .expect("the region is still registered");
    let pending = ctx.region_loops.remove(pos);
    ctx.current_block = plain;
    note(ctx, Route::RloopPlain);
    let r = lower(ctx);
    ctx.region_loops.push(pending);
    r?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_l);
    }
    ctx.current_block = merge;
    Ok(())
}

/// Did the guard match any receiver's SPILL word?
fn any_flag(ctx: &mut FnCtx<'_>, flags: &[String]) -> String {
    let mut acc = "false".to_string();
    for f in flags {
        acc = ctx.block().or(I1, &acc, f);
    }
    acc
}

pub(crate) fn end(ctx: &mut FnCtx<'_>, token: Option<u64>) {
    if let Some(t) = token {
        ctx.region_loops.retain(|p| p.token != t);
    }
}

/// Inside a call-free-by-construction fast clone of another tier the body is
/// not split: G-body contains calls, and those tiers discard a clone that
/// does.
fn in_call_free_clone(ctx: &FnCtx<'_>) -> bool {
    !ctx.class_field_loop_facts.is_empty()
        || !ctx.element_shape_loop_facts.is_empty()
        || !ctx.stable_packed_loop_facts.is_empty()
}

/// `lower_stmts`' hook: is `stmts` a registered region body?
pub(crate) fn pending_for(ctx: &FnCtx<'_>, stmts: &[Stmt]) -> Option<usize> {
    if ctx.region_loops.is_empty() || in_call_free_clone(ctx) {
        return None;
    }
    let ptr = stmts.as_ptr() as usize;
    ctx.region_loops
        .iter()
        .position(|p| p.body_ptr == ptr && p.body_len == stmts.len())
}

/// Lower a registered body: the prefix (body regions) once, then the split.
pub(crate) fn lower_split(
    ctx: &mut FnCtx<'_>,
    stmts: &[Stmt],
    idx: usize,
    lower_list: fn(&mut FnCtx<'_>, &[Stmt]) -> Result<()>,
) -> Result<()> {
    let split_at = ctx.region_loops[idx].split_at;
    let token = ctx.region_loops[idx].token;
    if split_at > 0 {
        lower_list(ctx, &stmts[..split_at])?;
        if ctx.block().is_terminated() {
            return Ok(());
        }
    }
    let tail = &stmts[split_at..];
    let mut receivers = ctx.region_loops[idx].receivers.clone();
    // A body region nested in a loop region: its G-tail leaves the loop
    // region (see `Pending::parent_valid`).
    let parent_valid = ctx.region_loops[idx].parent_valid.clone();
    // A body region's binding was declared by the prefix just lowered: if its
    // lowering gave it a special representation, the tail lowers plainly.
    if split_at > 0 && !receivers.iter().all(|rv| receiver_eligible(ctx, rv.recv)) {
        if let Some(v) = &parent_valid {
            ctx.block().store(I1, "false", v);
        }
        return lower_list(ctx, tail);
    }
    let inner = ctx.region_loops[idx].inner.clone();
    let valid_slot = ctx.region_loops[idx].valid_slot.clone();
    // A loop region with a nested body region re-checks on the dirty flag
    // (G-body's retry sets it).
    let recheck = match ctx.region_loops[idx].recheck {
        Recheck::None if inner.is_some() => Recheck::Dirty,
        r => r,
    };
    let retry = ctx.region_loops[idx].retry.clone();
    let bare = ctx.region_loops[idx].bare.clone();
    let bare_reads = ctx.region_loops[idx].bare_reads.clone();
    let number_local_uses = ctx.region_loops[idx].number_local_uses.clone();
    let declared_locals = ctx.region_loops[idx].declared_locals.clone();
    let planned_number_locals = ctx.region_loops[idx].number_locals.clone();
    let planned_entry_tests = ctx.region_loops[idx].entry_tests.clone();
    let trees = ctx.region_loops[idx].trees.clone();
    let dirty_slot = ctx.region_loops[idx].dirty_slot.clone();
    let arrs = ctx.region_loops[idx].arrays.clone();
    // The layouts F-body must serve: a loop region's split copy was chosen by
    // its preheader (one layout); a body region chooses per iteration, so it
    // carries the all-inline copy and, unless every key it names is stored
    // (then no spill word is ever published), the spill-reading copy.
    let modes: Vec<bool> = if valid_slot.is_some() {
        vec![ctx.region_loops[idx].spill_mode]
    } else if receivers
        .iter()
        .any(|r| r.stored_mask != (1u32 << r.keys.len()) - 1)
    {
        vec![false, true]
    } else {
        vec![false]
    };

    let fast = ctx.new_block("rloop.fast");
    let slow = ctx.new_block("rloop.slow");
    let join = ctx.new_block("rloop.join");
    let fast_l = ctx.block_label(fast);
    let slow_l = ctx.block_label(slow);
    let join_l = ctx.block_label(join);

    // Where the entry decision is emitted; its terminator is written LAST,
    // once `verify` has judged F-body.
    let mut decide;
    let mut decide_top: Option<(usize, String, String)> = None;
    // With a nested body region the top tests the dirty flag first: G-body
    // sets it to come back (`Pending::retry`); `decide_top` then carries
    // the valid flag.
    let dirty_first = inner.is_some();
    let mut direct: Option<(Sites, String)> = None;
    match &valid_slot {
        Some(slot) => {
            let v = ctx.block().load(I1, slot);
            if recheck == Recheck::None {
                decide = (ctx.current_block, v);
            } else {
                let rc = ctx.new_block("rloop.recheck");
                let rc_l = ctx.block_label(rc);
                let top = ctx.new_block("rloop.top");
                let top_l = ctx.block_label(top);
                let d_slot = dirty_slot.clone().expect("loop regions carry a dirty slot");
                let need = if dirty_first {
                    let d = ctx.block().load(I1, &d_slot);
                    ctx.block().cond_br(&d, &rc_l, &top_l);
                    ctx.current_block = top;
                    v
                } else {
                    ctx.block().cond_br(&v, &top_l, &slow_l);
                    ctx.current_block = top;
                    if recheck == Recheck::Dirty {
                        ctx.block().load(I1, &d_slot)
                    } else {
                        "true".to_string()
                    }
                };
                // Fresh without a re-check: straight into F-body. The
                // terminator of `top` is written at the end, like `decide`.
                let top_idx = ctx.current_block;
                ctx.current_block = rc;
                note(ctx, Route::RloopRecheck);
                let mut ok = "true".to_string();
                for rv in &receivers {
                    let recv_box = lower_recv(ctx, rv.recv)?;
                    let h = handle_of(ctx, &recv_box);
                    let sid = field_i32(ctx, &h, 4);
                    let exp = ctx.block().trunc(I64, &rv.word, I32);
                    // The spill copy runs only on flipped (spill) words.
                    let exp = if modes[0] {
                        ctx.block().xor(I32, &exp, FLIP_I32)
                    } else {
                        exp
                    };
                    let eq = ctx.block().icmp_eq(I32, &sid, &exp);
                    let eq = if rv.has_store {
                        let adm = store_admission(ctx, &h, false);
                        ctx.block().and(I1, &eq, &adm)
                    } else {
                        eq
                    };
                    ok = ctx.block().and(I1, &ok, &eq);
                }
                for a in &arrs {
                    let pass = arrays::emit_guard(ctx, a)?;
                    ok = ctx.block().and(I1, &ok, &pass);
                }
                if !planned_entry_tests.is_empty() {
                    let number_ok = emit_number_entry_tests(ctx, &planned_entry_tests)?;
                    ok = ctx.block().and(I1, &ok, &number_ok);
                }
                ctx.block().store(I1, &ok, slot);
                ctx.block().store(I1, "false", &d_slot);
                let rc_idx = ctx.current_block;
                decide_top = Some((top_idx, need, rc_l));
                decide = (rc_idx, ok);
            }
        }
        None if receivers.len() == 1 => {
            // Body region, one receiver: load the word now (F-body decodes
            // it); the rest of the guard is emitted at the end, as branches
            // straight into whichever F copies verified.
            let (sites, word) = emit_guard_word(ctx, &mut receivers[0]);
            receivers[0].word = word.clone();
            direct = Some((sites, word));
            stat(1, 1);
            decide = (ctx.current_block, String::new());
        }
        None => {
            // Body region: the full guard, every iteration.
            let mut all = "true".to_string();
            for rv in receivers.iter_mut() {
                let (word, pass, spill) = emit_guard(ctx, rv)?;
                rv.spill = spill;
                all = ctx.block().and(I1, &all, &pass);
                decode_slots(ctx, rv, &word);
            }
            stat(1, 1);
            decide = (ctx.current_block, all);
        }
    }

    let (number_locals, entry_tests) = if valid_slot.is_some() {
        (planned_number_locals, planned_entry_tests)
    } else {
        number_facts(
            ctx,
            tail,
            &receivers,
            &bare_reads,
            &number_local_uses,
            &declared_locals,
        )
    };
    if valid_slot.is_none() && direct.is_none() && !entry_tests.is_empty() {
        let number_ok = emit_number_entry_tests(ctx, &entry_tests)?;
        decide.1 = ctx.block().and(I1, &decide.1, &number_ok);
    }

    // F-body, once per layout; each copy is verified on its own IR.
    let mut copies: Vec<(String, bool)> = Vec::with_capacity(modes.len());
    for (ci, &mode) in modes.iter().enumerate() {
        let fb = if ci == 0 {
            fast
        } else {
            ctx.new_block("rloop.fast")
        };
        let fl = ctx.block_label(fb);
        ctx.current_block = fb;
        note(ctx, Route::RloopF);
        if let Some(d) = &retry {
            ctx.block().store(I1, "true", d);
        }
        let scan_start = ctx.func.num_blocks();
        let number_scope = ctx.next_loop_proof_scope_id();
        ctx.receiver_descriptors
            .materialize_number_locals(number_scope, &number_locals);
        ctx.region_loop_facts.push(Active {
            receivers: receivers.clone(),
            bare: bare.clone(),
            trees: trees.clone(),
            dirty_slot: dirty_slot.clone(),
            emitted: Vec::new(),
            handles: Vec::new(),
            spill: mode,
            arrays: arrs.clone(),
            emitted_arr: Vec::new(),
        });
        let r = match &inner {
            Some(ib) => {
                ctx.region_loops.push((**ib).clone());
                let j = ctx.region_loops.len() - 1;
                let r = lower_split(ctx, tail, j, lower_list);
                ctx.region_loops.truncate(j);
                r
            }
            None => lower_list(ctx, tail),
        };
        let active = ctx.region_loop_facts.pop().expect("pushed above");
        ctx.receiver_descriptors.dematerialize_scope(number_scope);
        r?;
        if !ctx.block().is_terminated() {
            ctx.block().br(&join_l);
        }
        let scan_end = ctx.func.num_blocks();
        // The element reads are judged for JS like every bare access, and
        // for collections on their own (their base is an address).
        let all_emitted: Vec<(usize, usize)> = active
            .emitted
            .iter()
            .chain(active.emitted_arr.iter())
            .copied()
            .collect();
        let ok = verify(ctx, fb, scan_start, scan_end, &all_emitted)
            && arrays::verify_arrays(
                ctx,
                fb,
                scan_start,
                scan_end,
                &active.emitted_arr,
                recheck,
                dirty_slot.as_deref(),
                valid_slot.as_deref(),
            );
        if !ok {
            stat(4, 1);
            if std::env::var("PERRY_REGION_DIAG").as_deref() == Ok("2") {
                eprintln!(
                    "[perry region] F-body discarded by the verifier in {}",
                    ctx.func.name
                );
            }
        }
        copies.push((fl, ok));
    }
    let ok = copies[0].1;
    let _ = &fast_l;

    // G-body: today's lowering. A loop region with nothing between
    // iterations that can invalidate its facts (no re-check) never leaves F
    // once its split loop is entered — the preheader's plain loop is its G —
    // so the split loop carries no G copy. A nested body region's G-tail
    // leaves the loop region, so that loop keeps its G.
    let g_dead = ok && valid_slot.is_some() && recheck == Recheck::None && inner.is_none();
    ctx.current_block = slow;
    if g_dead {
        ctx.block().unreachable();
    } else {
        note(ctx, Route::RloopG);
        if let Some(v) = &parent_valid {
            ctx.block().store(I1, "false", v);
        }
        // A loop region's G-body is today's loop body, which splits at the
        // nested body region.
        match &inner {
            Some(ib) => {
                let mut ib = (**ib).clone();
                ib.retry = dirty_slot.clone();
                // G-body runs only while the loop is not valid.
                ib.parent_valid = None;
                ctx.region_loops.push(ib);
                let j = ctx.region_loops.len() - 1;
                let r = lower_split(ctx, tail, j, lower_list);
                ctx.region_loops.truncate(j);
                r?;
            }
            None => lower_list(ctx, tail)?,
        }
        if !ctx.block().is_terminated() {
            ctx.block().br(&join_l);
        }
    }

    if let Some((top_idx, need, rc_l)) = decide_top {
        ctx.current_block = top_idx;
        if ok && dirty_first {
            ctx.block().cond_br(&need, &fast_l, &slow_l);
        } else if ok {
            ctx.block().cond_br(&need, &rc_l, &fast_l);
        } else {
            ctx.block().br(&slow_l);
        }
    }
    ctx.current_block = decide.0;
    if let Some((sites, word)) = &direct {
        let inline_t = if copies[0].1 {
            copies[0].0.clone()
        } else {
            slow_l.clone()
        };
        let spill_t = if copies.len() == 2 && copies[1].1 {
            copies[1].0.clone()
        } else {
            slow_l.clone()
        };
        let rv = receivers[0].clone();
        emit_body_guard_direct(
            ctx,
            &rv,
            sites,
            word,
            &entry_tests,
            &inline_t,
            &spill_t,
            &slow_l,
        )?;
    } else if copies.len() == 2 {
        // Body region: the guard passed -> pick the copy for the word's layout.
        let target = |c: &(String, bool)| if c.1 { c.0.clone() } else { slow_l.clone() };
        let (inline_t, spill_t) = (target(&copies[0]), target(&copies[1]));
        if !copies[0].1 && !copies[1].1 {
            ctx.block().br(&slow_l);
        } else {
            let mode_b = ctx.new_block("rloop.mode");
            let mode_l = ctx.block_label(mode_b);
            ctx.block().cond_br(&decide.1, &mode_l, &slow_l);
            ctx.current_block = mode_b;
            let flags: Vec<String> = receivers.iter().map(|r| r.spill.clone()).collect();
            let any = any_flag(ctx, &flags);
            ctx.block().cond_br(&any, &spill_t, &inline_t);
        }
    } else if g_dead {
        ctx.block().br(&fast_l);
    } else if ok {
        ctx.block().cond_br(&decide.1, &fast_l, &slow_l);
    } else {
        ctx.block().br(&slow_l);
    }
    ctx.current_block = join;
    let _ = token;
    Ok(())
}
