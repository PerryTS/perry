//! Flow analysis: locals safe to treat as native-i32 ("integer-valued") even
//! though (at least) one of their writes is a *possibly out-of-bounds* integer
//! typed-array element read.
//!
//! ## Motivation (bcryptjs `_encipher` Feistel accumulators)
//!
//! ```ignore
//! function _encipher(lr: Int32Array, off: number, P: Int32Array, S: Int32Array) {
//!   let l = lr[off], r = lr[off + 1];   // int typed-array reads (index UNBOUNDED)
//!   l ^= P[0];                          // only ever bitwise-updated
//!   ... S[l >>> 24] ... S[l & 0xff] ...  // only ever read in bitwise / index ctx
//!   lr[off + 1] = l;                    // stored back into an int typed array
//! }
//! ```
//!
//! `l` / `r` are logically int32, but their declared type is erased to `Any`
//! (the `let l = lr[off]` inference does not propagate the element type). The
//! existing `collect_integer_locals` only admits a typed-array element read as
//! integer-valued when the index is *statically proven in-bounds*
//! (`collect_int_ta_load_let_ids`); an unbounded `lr[off]` is rejected, so `l`
//! never enters `integer_locals`, never gets an i32 shadow slot, and every
//! `l ^ x` / `S[l >>> 24]` pays an `fptosi`/`sitofp` round-trip.
//!
//! ## The soundness trap
//!
//! An `Int32Array` element read is int32 **only in-bounds**. An OOB / negative /
//! fractional index yields **`undefined`** (a NaN-boxed value), NOT an integer.
//! (`Uint8ArrayGet` is safely integer-valued because its accessor returns `0`
//! OOB — a general typed-array read does not.) So marking such a local i32
//! unconditionally would let `let x = S[oob]; console.log(x)` print a number
//! (`fptosi(undefined)` garbage / the seeded `0`) where JS prints `undefined`.
//!
//! ## What makes it sound
//!
//! A local is admitted here **only** when BOTH hold:
//!
//! 1. **Every write** produces an i32-representable value OR is an int-kind
//!    typed-array element read (`Int8/Uint8/Uint8Clamped/Int16/Uint16/Int32` —
//!    NOT `Uint32`, NOT the float / bigint kinds, NOT a plain-array `[]`):
//!    a bitwise op (`& | ^ << >> >>>`), `~`, `Math.imul`, an i32 literal,
//!    `undefined` (the hoisted-`var` seed — `ToInt32(undefined) == 0`, the
//!    slot's seed value), or a `Uint8ArrayGet`/`BufferIndexGet`. NOT `*`
//!    (a single product can exceed 2^53 and round). Additive `+`/`-` is
//!    admitted ONLY under the wrap-i32 extension (`additive_write_admissible`,
//!    representation-selection Phase 2): a STRAIGHT-LINE (never in-loop)
//!    Add/Sub tree over exact-i32 operands (in-bounds-PROVEN int-TA reads,
//!    literals, bitwise results, other candidates), where the slot carries the
//!    ToInt32 image of the true value — sound because rule (2) makes the image
//!    the only thing ever observed, and bounded straight-line chains keep the
//!    true f64 intermediates exactly representable.
//! 2. **Every observation** is in an integer-coercing context — the direct
//!    operand of a bitwise binary/unary op, or the value stored into an
//!    int-kind typed-array / `Uint8Array` / `Buffer` element. NEVER a context
//!    where `undefined`-vs-integer is distinguishable (array index, additive
//!    operand, `%`/`/`, comparison, call argument, `return`, `console.log`,
//!    `String()`, `typeof`, property/field/plain-array store, `+` string, …).
//!
//! Under (2) the local's runtime value is *always* fed through `ToInt32`
//! (`ToInt32(undefined) == 0`), and the i32 slot is seeded with the same `0`
//! for an OOB read (see the NaN-safe seed in `stmt/let_stmt.rs`), so the two
//! representations are byte-for-byte indistinguishable — while the fast i32
//! chain is unlocked. As soon as a value passes through one bitwise op the
//! `undefined`→`0` collapse has already happened identically on both paths, so
//! no transitive constraint on *downstream* locals is needed.
//!
//! ## Under-approximation
//!
//! This is deliberately conservative (the #6794 family rule: a correct 1.1×
//! beats an unsound 1.42×). It requires the local to be `let`-declared (params
//! excluded — their incoming argument is an unmodeled write), rejects `++`/`--`
//! targets, rejects any local referenced inside a closure body, and does NOT
//! chase copy chains (`m = l`). Anything unproven is simply left as f64.
//! Rule (1)'s strict arm is judged per write structurally; the wrap-i32
//! additive arm references other candidates, so admission and the rule-(2)
//! walk run to a (small, monotone-shrinking) fixpoint.
//!
//! Gated by `PERRY_INT_VALUED_LOCALS` (default on; `=0`/`off`/`false` disables
//! for A/B bisection — keyed into the object cache in `object_cache.rs`).

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Expr, Param, Stmt, UnaryOp};

/// `PERRY_INT_VALUED_LOCALS` gate. Enabled by default; `=0`/`off`/`false`
/// disables the analysis (returns an empty set), reverting the affected locals
/// to the f64 representation. Mirrors the sibling codegen fast-path env gates.
pub fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_INT_VALUED_LOCALS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Integer typed-array element kinds whose value round-trips through a signed
/// i32 slot AND whose OOB read (`undefined`) is `ToInt32`-equal to `0`.
/// Excludes `Uint32Array` (upper half does not fit a signed i32) and the
/// float / bigint kinds. Mirrors `i32_locals::typed_array_kind_elem_fits_i32`
/// but keyed on the class name.
fn is_int_elem_typed_array_class(name: &str) -> bool {
    matches!(
        name,
        "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
    )
}

/// True when `object` is a local/param whose declared type is an int-kind
/// typed array (so `object[i]` reads an integer-or-`undefined`, and
/// `object[i] = v` coerces `v` via `ToInt32`/`ToUint8`/…).
fn receiver_is_int_kind_ta(object: &Expr, types: &HashMap<u32, HirType>) -> bool {
    let Expr::LocalGet(id) = object else {
        return false;
    };
    matches!(types.get(id), Some(HirType::Named(name)) if is_int_elem_typed_array_class(name))
}

/// A bare int-kind typed-array element read `S[idx]` (any index — possibly OOB).
fn is_int_kind_ta_read(e: &Expr, types: &HashMap<u32, HirType>) -> bool {
    matches!(e, Expr::IndexGet { object, .. } if receiver_is_int_kind_ta(object, types))
}

fn is_bitwise_binop(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::UShr
    )
}

/// Rule (1): a write whose value is *always* a genuine 32-bit integer, OR a
/// possibly-OOB int typed-array read (integer in-bounds, `undefined` OOB — made
/// observationally equivalent to `0` by rule (2)). Rejects additive / `*` /
/// `/` / `%` (i32 overflow / non-integer), copies, calls, and everything else.
fn write_is_i32_producing_safe(e: &Expr, types: &HashMap<u32, HirType>) -> bool {
    match e {
        Expr::Integer(n) => super::i32_locals::integer_literal_fits_i32(*n),
        // Hoisted-`var` seed (`var n, l = lr[off]` lowers as `Let{l,
        // Undefined}` + `Let{l, lr[off]}`): `ToInt32(undefined) == 0`, which
        // is exactly the value the i32 slot is seeded with, and rule (2)
        // already guarantees every observation is ToInt32-coercing — so an
        // `undefined` write is indistinguishable from the 0 it becomes.
        Expr::Undefined => true,
        // Byte reads: `0` OOB, always integer.
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => true,
        // Int-kind typed-array element read (possibly OOB → `undefined`).
        Expr::IndexGet { object, .. } => receiver_is_int_kind_ta(object, types),
        // Bitwise ops coerce both operands to int32 and yield int32 regardless
        // of operand shapes — no operand check needed.
        Expr::Binary { op, .. } => is_bitwise_binop(*op),
        Expr::Unary {
            op: UnaryOp::BitNot,
            ..
        } => true,
        Expr::MathImul(_, _) => true,
        _ => false,
    }
}

/// One-pass structural facts gathered before eligibility is decided.
#[derive(Default)]
struct Facts<'a> {
    /// Every write (Let init + `LocalSet` rhs) per local, with a flag for
    /// "this write sits inside a loop" (additive writes must be straight-line
    /// — see the wrap-i32 extension below).
    writes: HashMap<u32, Vec<(&'a Expr, bool)>>,
    /// Locals introduced by a `Stmt::Let` (candidates must be `let`-declared —
    /// params carry an unmodeled incoming-argument write).
    let_declared: HashSet<u32>,
    /// Locals with ≥1 int-kind typed-array element read write (the seed: only
    /// these need this analysis; a local with no such write is already handled
    /// by `collect_integer_locals` when it qualifies).
    seeded: HashSet<u32>,
    /// Targets of `++`/`--` — excluded (the update's `± 1` can overflow i32 and
    /// is not modeled as a write here).
    update_targets: HashSet<u32>,
    /// Locals referenced (read or written) anywhere inside a closure body —
    /// excluded (a captured local cannot use the i32 slot).
    closure_refs: HashSet<u32>,
}

/// Wrap-i32 extension (representation-selection Phase 2): a write that is an
/// `Add`/`Sub` tree over EXACT-i32 operands. `ToInt32(a + b) ==
/// wrap32(ToInt32(a) + ToInt32(b))` whenever the true f64 intermediates stay
/// exactly representable, so a local whose every observation is
/// ToInt32-coercing may carry the WRAPPED image of an additive chain — the
/// bcryptjs `n += S[...]` Feistel accumulator. Soundness needs three extra
/// legs, all enforced by the caller:
/// - operands must be undefined-free EXACT int32 values — an in-bounds-PROVEN
///   int-TA read (static window < known constant length), an i32 literal, a
///   bitwise/`~`/`Math.imul` result, a byte read, or another wrap-i32/strict
///   candidate (whose slot holds the ToInt32 image by induction);
/// - the additive write must be STRAIGHT-LINE (not inside any loop): a
///   loop-carried additive chain grows the true float unboundedly until f64
///   addition rounds (≥ 2^53) and the wrapped image diverges. Straight-line
///   chains are bounded by the body's statement count (≪ 2^20 additions), so
///   every true intermediate stays < 2^52 — exact;
/// - candidate reads at additive-operand positions are blessed by the
///   observation walk (`observe_additive_rhs`), everything else keeps the
///   strict coercing rule (in particular an INDEX position is still
///   disqualifying — a wrapped image used as an index could alias a
///   different element than the true out-of-range value).
fn additive_write_admissible(
    e: &Expr,
    types: &HashMap<u32, HirType>,
    ta_lens: &HashMap<u32, i64>,
    pool: &HashSet<u32>,
) -> bool {
    match e {
        Expr::Integer(n) => super::i32_locals::integer_literal_fits_i32(*n),
        Expr::LocalGet(id) => pool.contains(id),
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => true,
        // In-bounds-proven int-kind typed-array read: never `undefined`.
        Expr::IndexGet { object, index } => {
            receiver_is_int_kind_ta(object, types)
                && matches!(object.as_ref(), Expr::LocalGet(arr)
                if matches!(
                    (ta_lens.get(arr), super::integer_locals::static_index_window(index)),
                    (Some(len), Some((lo, hi))) if lo >= 0 && hi < *len
                ))
        }
        // Exact ToInt32/ToUint32 producers regardless of operand shape.
        Expr::Binary { op, .. } if is_bitwise_binop(*op) => true,
        Expr::Unary {
            op: UnaryOp::BitNot,
            ..
        } => true,
        Expr::MathImul(_, _) => true,
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            additive_write_admissible(left, types, ta_lens, pool)
                && additive_write_admissible(right, types, ta_lens, pool)
        }
        _ => false,
    }
}

pub fn collect_int_valued_ta_locals(
    stmts: &[Stmt],
    params: &[Param],
    binding_types: &HashMap<u32, HirType>,
    extra_ta_lens: &HashMap<u32, i64>,
) -> HashSet<u32> {
    // Declared-type map (params + let bindings), used to classify typed-array
    // receivers. Params are included so `lr: Int32Array` resolves.
    let mut types: HashMap<u32, HirType> = binding_types.clone();
    for p in params {
        types.entry(p.id).or_insert_with(|| p.ty.clone());
    }
    // Constant typed-array lengths for the wrap-i32 in-bounds proof: in-body
    // literal-length const views plus caller-supplied lengths (spec-ABI
    // `TaPtr` params carry theirs from the call-site pre-pass).
    let mut ta_lens = super::integer_locals::collect_const_int_ta_views(stmts);
    for (id, len) in extra_ta_lens {
        ta_lens.entry(*id).or_insert(*len);
    }

    let mut facts = Facts::default();
    collect_facts(stmts, &types, false, &mut facts);

    // Rule (1) admission. A candidate is a `let`-declared local with ≥1
    // int-TA-read write, whose EVERY write is i32-producing-safe (or, in the
    // wrap-i32 extension, a straight-line additive tree over exact operands),
    // that is not a `++`/`--` target and not referenced in a closure.
    // Additive operands may reference OTHER candidates, so admission runs to a
    // fixpoint from an optimistic pool.
    let base_ok = |id: u32, facts: &Facts<'_>| {
        facts.let_declared.contains(&id)
            && facts.seeded.contains(&id)
            && !facts.update_targets.contains(&id)
            && !facts.closure_refs.contains(&id)
    };
    let mut pool: HashSet<u32> = facts
        .writes
        .keys()
        .copied()
        .filter(|id| base_ok(*id, &facts))
        .collect();
    loop {
        let before = pool.len();
        let snapshot = pool.clone();
        pool.retain(|id| {
            facts.writes[id].iter().all(|(w, in_loop)| {
                write_is_i32_producing_safe(w, &types)
                    || (!in_loop && additive_write_admissible(w, &types, &ta_lens, &snapshot))
            })
        });
        if pool.len() == before {
            break;
        }
    }
    let mut candidates = pool;
    if candidates.is_empty() {
        return candidates;
    }

    // Rule (2) observation check: disqualify any candidate read in a
    // non-`ToInt32`-coercing position (additive-operand positions inside a
    // candidate's own admissible additive write are blessed). Disqualifying a
    // candidate can invalidate another candidate's additive operand, so this
    // also runs to a fixpoint.
    loop {
        let mut disqualified: HashSet<u32> = HashSet::new();
        let additive_ctx = AdditiveCtx {
            ta_lens: &ta_lens,
            pool: &candidates,
        };
        observe_stmts(stmts, &candidates, &types, &additive_ctx, &mut disqualified);
        if disqualified.is_empty() {
            break;
        }
        candidates.retain(|id| !disqualified.contains(id));
        // A shrunken pool can turn a previously-admissible additive write
        // inadmissible — re-run rule (1) against the new pool.
        let mut changed = true;
        while changed {
            let before = candidates.len();
            let snapshot = candidates.clone();
            candidates.retain(|id| {
                facts.writes[id].iter().all(|(w, in_loop)| {
                    write_is_i32_producing_safe(w, &types)
                        || (!in_loop && additive_write_admissible(w, &types, &ta_lens, &snapshot))
                })
            });
            changed = candidates.len() != before;
        }
        if candidates.is_empty() {
            return candidates;
        }
    }
    candidates
}

/// Context for the additive-operand blessing in the observation walk.
struct AdditiveCtx<'a> {
    ta_lens: &'a HashMap<u32, i64>,
    pool: &'a HashSet<u32>,
}

// ---------------------------------------------------------------------------
// Fact collection (writes / seeds / update targets / closure refs).
// ---------------------------------------------------------------------------

fn collect_facts<'a>(
    stmts: &'a [Stmt],
    types: &HashMap<u32, HirType>,
    in_loop: bool,
    facts: &mut Facts<'a>,
) {
    for s in stmts {
        match s {
            Stmt::Let { id, init, .. } => {
                facts.let_declared.insert(*id);
                if let Some(e) = init {
                    record_write(*id, e, types, in_loop, facts);
                    collect_facts_expr(e, types, in_loop, facts);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => collect_facts_expr(e, types, in_loop, facts),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    collect_facts_expr(e, types, in_loop, facts);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_facts_expr(condition, types, in_loop, facts);
                collect_facts(then_branch, types, in_loop, facts);
                if let Some(eb) = else_branch {
                    collect_facts(eb, types, in_loop, facts);
                }
            }
            Stmt::While { condition, body } => {
                collect_facts_expr(condition, types, true, facts);
                collect_facts(body, types, true, facts);
            }
            Stmt::DoWhile { body, condition } => {
                collect_facts(body, types, true, facts);
                collect_facts_expr(condition, types, true, facts);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    collect_facts(std::slice::from_ref(i.as_ref()), types, in_loop, facts);
                }
                if let Some(c) = condition {
                    collect_facts_expr(c, types, true, facts);
                }
                if let Some(u) = update {
                    collect_facts_expr(u, types, true, facts);
                }
                collect_facts(body, types, true, facts);
            }
            Stmt::Labeled { body, .. } => {
                collect_facts(std::slice::from_ref(body.as_ref()), types, in_loop, facts);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_facts(body, types, in_loop, facts);
                if let Some(c) = catch {
                    collect_facts(&c.body, types, in_loop, facts);
                }
                if let Some(f) = finally {
                    collect_facts(f, types, in_loop, facts);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_facts_expr(discriminant, types, in_loop, facts);
                for case in cases {
                    if let Some(t) = &case.test {
                        collect_facts_expr(t, types, in_loop, facts);
                    }
                    collect_facts(&case.body, types, in_loop, facts);
                }
            }
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }
}

fn record_write<'a>(
    id: u32,
    rhs: &'a Expr,
    types: &HashMap<u32, HirType>,
    in_loop: bool,
    facts: &mut Facts<'a>,
) {
    facts.writes.entry(id).or_default().push((rhs, in_loop));
    if is_int_kind_ta_read(rhs, types) {
        facts.seeded.insert(id);
    }
}

fn collect_facts_expr<'a>(
    e: &'a Expr,
    types: &HashMap<u32, HirType>,
    in_loop: bool,
    facts: &mut Facts<'a>,
) {
    match e {
        Expr::LocalSet(id, rhs) => {
            record_write(*id, rhs, types, in_loop, facts);
        }
        Expr::Update { id, .. } => {
            facts.update_targets.insert(*id);
        }
        Expr::Closure { .. } => {
            // Everything a closure touches is excluded from candidacy.
            collect_closure_refs(e, &mut facts.closure_refs);
            // Still descend to record nested `Update` targets on ENCLOSING
            // locals (defensive; those ids are already in `closure_refs`).
            perry_hir::walker::walk_expr_children(e, &mut |c| {
                collect_facts_expr(c, types, in_loop, facts)
            });
            return;
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| collect_facts_expr(c, types, in_loop, facts));
}

/// Collect every local id read or written anywhere inside `e` (used to exclude
/// closure-touched candidates). Walks closure bodies (statements) too.
fn collect_closure_refs(e: &Expr, out: &mut HashSet<u32>) {
    match e {
        Expr::LocalGet(id) | Expr::Update { id, .. } => {
            out.insert(*id);
        }
        Expr::LocalSet(id, value) => {
            out.insert(*id);
            collect_closure_refs(value, out);
        }
        Expr::Closure { body, .. } => {
            for s in body {
                collect_closure_refs_stmt(s, out);
            }
            perry_hir::walker::walk_expr_children(e, &mut |c| collect_closure_refs(c, out));
        }
        _ => {
            perry_hir::walker::walk_expr_children(e, &mut |c| collect_closure_refs(c, out));
        }
    }
}

fn collect_closure_refs_stmt(s: &Stmt, out: &mut HashSet<u32>) {
    match s {
        Stmt::Let { id, init, .. } => {
            out.insert(*id);
            if let Some(e) = init {
                collect_closure_refs(e, out);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => collect_closure_refs(e, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                collect_closure_refs(e, out);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_closure_refs(condition, out);
            for s in then_branch {
                collect_closure_refs_stmt(s, out);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    collect_closure_refs_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } => {
            collect_closure_refs(condition, out);
            for s in body {
                collect_closure_refs_stmt(s, out);
            }
        }
        Stmt::DoWhile { body, condition } => {
            for s in body {
                collect_closure_refs_stmt(s, out);
            }
            collect_closure_refs(condition, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                collect_closure_refs_stmt(i, out);
            }
            if let Some(c) = condition {
                collect_closure_refs(c, out);
            }
            if let Some(u) = update {
                collect_closure_refs(u, out);
            }
            for s in body {
                collect_closure_refs_stmt(s, out);
            }
        }
        Stmt::Labeled { body, .. } => collect_closure_refs_stmt(body, out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_closure_refs_stmt(s, out);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    collect_closure_refs_stmt(s, out);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    collect_closure_refs_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_closure_refs(discriminant, out);
            for case in cases {
                if let Some(t) = &case.test {
                    collect_closure_refs(t, out);
                }
                for s in &case.body {
                    collect_closure_refs_stmt(s, out);
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Rule (2): observation check. `coercing` = whether a bare `LocalGet(cand)` in
// THIS position is fed through `ToInt32` (so an OOB `undefined` reads as `0`).
// ---------------------------------------------------------------------------

fn observe_stmts(
    stmts: &[Stmt],
    cands: &HashSet<u32>,
    types: &HashMap<u32, HirType>,
    additive: &AdditiveCtx<'_>,
    disq: &mut HashSet<u32>,
) {
    for s in stmts {
        match s {
            Stmt::Let { id, init, .. } => {
                if let Some(e) = init {
                    // Same additive blessing as the `LocalSet` arm — a
                    // candidate's Let-init may be an admissible additive tree.
                    if cands.contains(id)
                        && additive_write_admissible(e, types, additive.ta_lens, additive.pool)
                    {
                        observe_additive_rhs(e, cands, types, additive, disq);
                    } else {
                        observe(e, false, cands, types, additive, disq);
                    }
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => observe(e, false, cands, types, additive, disq),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    observe(e, false, cands, types, additive, disq);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                observe(condition, false, cands, types, additive, disq);
                observe_stmts(then_branch, cands, types, additive, disq);
                if let Some(eb) = else_branch {
                    observe_stmts(eb, cands, types, additive, disq);
                }
            }
            Stmt::While { condition, body } => {
                observe(condition, false, cands, types, additive, disq);
                observe_stmts(body, cands, types, additive, disq);
            }
            Stmt::DoWhile { body, condition } => {
                observe_stmts(body, cands, types, additive, disq);
                observe(condition, false, cands, types, additive, disq);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    observe_stmts(
                        std::slice::from_ref(i.as_ref()),
                        cands,
                        types,
                        additive,
                        disq,
                    );
                }
                if let Some(c) = condition {
                    observe(c, false, cands, types, additive, disq);
                }
                if let Some(u) = update {
                    observe(u, false, cands, types, additive, disq);
                }
                observe_stmts(body, cands, types, additive, disq);
            }
            Stmt::Labeled { body, .. } => {
                observe_stmts(
                    std::slice::from_ref(body.as_ref()),
                    cands,
                    types,
                    additive,
                    disq,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                observe_stmts(body, cands, types, additive, disq);
                if let Some(c) = catch {
                    observe_stmts(&c.body, cands, types, additive, disq);
                }
                if let Some(f) = finally {
                    observe_stmts(f, cands, types, additive, disq);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                observe(discriminant, false, cands, types, additive, disq);
                for case in cases {
                    if let Some(t) = &case.test {
                        observe(t, false, cands, types, additive, disq);
                    }
                    observe_stmts(&case.body, cands, types, additive, disq);
                }
            }
            _ => {}
        }
    }
}

fn observe(
    e: &Expr,
    coercing: bool,
    cands: &HashSet<u32>,
    types: &HashMap<u32, HirType>,
    additive: &AdditiveCtx<'_>,
    disq: &mut HashSet<u32>,
) {
    match e {
        Expr::LocalGet(id) => {
            if cands.contains(id) && !coercing {
                disq.insert(*id);
            }
        }
        // A `++`/`--` target is already excluded at admission; if one slips
        // through as a read, it is not a coercing observation.
        Expr::Update { id, .. } => {
            if cands.contains(id) {
                disq.insert(*id);
            }
        }
        // Bitwise binary: both operands are `ToInt32`-coerced.
        Expr::Binary { op, left, right } => {
            let c = is_bitwise_binop(*op);
            observe(left, c, cands, types, additive, disq);
            observe(right, c, cands, types, additive, disq);
        }
        // `~x` coerces its operand via `ToInt32`; `-x`/`+x`/`!x` do NOT make an
        // `undefined`-vs-integer distinction disappear.
        Expr::Unary { op, operand } => {
            observe(
                operand,
                matches!(op, UnaryOp::BitNot),
                cands,
                types,
                additive,
                disq,
            );
        }
        // Store into a typed-array element: the value is coerced (`ToInt32` /
        // `ToUint8` / …) iff the receiver is an int-kind typed array. The index
        // is NOT a coercing position (`S[l]` with `l == undefined` differs from
        // `S[0]`).
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            observe(target, false, cands, types, additive, disq);
            observe(key, false, cands, types, additive, disq);
            observe(receiver, false, cands, types, additive, disq);
            let store_coercing =
                receiver_is_int_kind_ta(receiver, types) || receiver_is_int_kind_ta(target, types);
            observe(value, store_coercing, cands, types, additive, disq);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            observe(object, false, cands, types, additive, disq);
            observe(index, false, cands, types, additive, disq);
            observe(
                value,
                receiver_is_int_kind_ta(object, types),
                cands,
                types,
                additive,
                disq,
            );
        }
        // Byte stores clamp/mask the value through `ToUint8` (`undefined` → 0),
        // so the stored value position is coercing.
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => {
            observe(array, false, cands, types, additive, disq);
            observe(index, false, cands, types, additive, disq);
            observe(value, true, cands, types, additive, disq);
        }
        Expr::BufferIndexSet {
            buffer,
            index,
            value,
        } => {
            observe(buffer, false, cands, types, additive, disq);
            observe(index, false, cands, types, additive, disq);
            observe(value, true, cands, types, additive, disq);
        }
        // Assignment rhs: a bare `LocalGet(cand)` here is a copy (not modeled),
        // so it is non-coercing. Nested bitwise sub-expressions re-establish
        // coercing-ness for their own operands. EXCEPTION (wrap-i32): a
        // candidate's own admissible additive write blesses candidate reads
        // at its Add/Sub-operand positions.
        Expr::LocalSet(target, value) => {
            if cands.contains(target)
                && additive_write_admissible(value, types, additive.ta_lens, additive.pool)
            {
                observe_additive_rhs(value, cands, types, additive, disq);
            } else {
                observe(value, false, cands, types, additive, disq);
            }
        }
        // Closure-touched candidates are already excluded; do not descend.
        Expr::Closure { .. } => {}
        // Every other position is non-coercing: recurse with `coercing = false`
        // so any candidate read there disqualifies it.
        _ => {
            perry_hir::walker::walk_expr_children(e, &mut |c| {
                observe(c, false, cands, types, additive, disq)
            });
        }
    }
}

/// Observation walk for an ADMISSIBLE additive write's RHS: candidate reads at
/// Add/Sub-operand chain positions are blessed (the slot's ToInt32 image is
/// exact there — see `additive_write_admissible`); every other nested position
/// (typed-array INDEX expressions, unmodeled shapes) keeps the strict rule.
fn observe_additive_rhs(
    e: &Expr,
    cands: &HashSet<u32>,
    types: &HashMap<u32, HirType>,
    additive: &AdditiveCtx<'_>,
    disq: &mut HashSet<u32>,
) {
    match e {
        // Blessed operand read of a candidate (or a plain non-candidate read
        // — no constraint either way).
        Expr::LocalGet(_) | Expr::Integer(_) | Expr::Number(_) => {}
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            observe_additive_rhs(left, cands, types, additive, disq);
            observe_additive_rhs(right, cands, types, additive, disq);
        }
        // Bitwise/`~`/`imul` operands are ToInt32/ToUint32-coerced — the
        // strict walk already treats those positions as coercing.
        Expr::Binary { op, left, right } if is_bitwise_binop(*op) => {
            observe(left, true, cands, types, additive, disq);
            observe(right, true, cands, types, additive, disq);
        }
        Expr::Unary {
            op: UnaryOp::BitNot,
            operand,
        } => {
            observe(operand, true, cands, types, additive, disq);
        }
        Expr::MathImul(left, right) => {
            observe(left, true, cands, types, additive, disq);
            observe(right, true, cands, types, additive, disq);
        }
        // In-bounds-proven typed-array read operand: its INDEX is walked with
        // the strict (non-coercing) rule — a wrapped image used as an index
        // would be disqualifying, exactly as in ordinary code.
        Expr::IndexGet { object, index } => {
            observe(object, false, cands, types, additive, disq);
            observe(index, false, cands, types, additive, disq);
        }
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => {
            observe(e, false, cands, types, additive, disq);
        }
        // Anything else in an "admissible" tree would be a grammar bug —
        // fall back to the strict walk (disqualifying, never unsound).
        _ => observe(e, false, cands, types, additive, disq),
    }
}

#[cfg(test)]
mod tests;
