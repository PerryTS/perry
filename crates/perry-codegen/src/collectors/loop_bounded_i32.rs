//! Repsel Phase 1 widening (#7110): the **monotone loop-induction** i32 range
//! proof.
//!
//! ## The hole this fills
//!
//! `stmt/let_stmt.rs`'s canonical-i32 gate admits a proven-integer local only
//! when it is *index-used*, *strictly-i32-bounded*, `>>> 0`-written, or an
//! int-typed-array accumulator. A loop counter satisfies none of those:
//!
//! ```text
//! for (let i = 0; i < 1000000; i++) { sum = sum + 1; }
//! ```
//!
//! `i` is not index-used (nothing is indexed), and it is not
//! `strictly_i32_bounded` because `i++` unconditionally disqualifies a local
//! there (#6072 — `x++` is `x = x + 1` with full f64 semantics, so an
//! *unbounded* `++` really can leave i32 range). Add one `a[i]` read and `i`
//! promotes; the promotion turned on the presence of an array, not on any
//! property of `i`.
//!
//! ## The proof
//!
//! For a local `v` this analysis proves a **closed interval** that `v` can
//! never leave, from the loop guard that dominates every write to it:
//!
//! 1. `v` has exactly one declaration, whose initialiser is an integer literal
//!    `I` inside i32 range.
//! 2. Every write to `v` anywhere in the function is a **step** — `v++`,
//!    `v += k`, `v = v + k` (or the `--`/`-=`/`v = v - k` mirror) with `k` a
//!    non-negative integer constant — and every such step sits **directly in
//!    the body or update of a loop whose condition guards `v`**, with no
//!    intervening loop and no intervening closure.
//! 3. That loop's condition has a top-level `&&`-spine conjunct `v < B`,
//!    `v <= B`, `v > B` or `v >= B` (either operand order) with `B` an
//!    integer constant in i32 range.
//! 4. The step direction agrees with the guard: `<`/`<=` admits only
//!    increments, `>`/`>=` only decrements. (`for (let i = 0; i < 10; i--)`
//!    runs away downwards and must not be admitted.)
//!
//! Then, writing `S` for the sum of the `k`s of all step sites at that loop
//! level (each executes at most once per iteration — that is what "no
//! intervening loop" buys):
//!
//! * increments: every step is reached only after the guard `v < B` was
//!   observed true this iteration, so `v <= B - 1` before it and
//!   `v <= B - 1 + S` after; `v` never decreases, so `v >= I`. The interval is
//!   `[I, B - 1 + S]` (`[I, B + S]` for `<=`).
//! * decrements: the mirror, `[B + 1 - S, I]` (`[B - S, I]` for `>=`).
//!
//! Both endpoints are compile-time integers; the local is admitted only when
//! **both fit i32**. That is a genuine range argument, not the compatibility
//! bound that `integer_locals ∩ index_used_locals` rests on (see the range
//! soundness audit in `expr/slot_rep.rs`): it introduces no new overflow
//! surface, because there is no reachable state in which the value leaves i32.
//!
//! The same argument is already trusted elsewhere in codegen — `stmt/loops.rs`
//! allocates a *parallel* i32 shadow for a constant-bounded counter on exactly
//! this reasoning ("with a CONSTANT bound the counter provably stays in i32
//! range"). This module lifts it to a Let-site fact so the counter can take
//! canonical storage (i32 slot only, no double slot, no dual writes) instead of
//! a shadow that has to be kept in sync with a boxed double.
//!
//! ## Bounded accumulators (#7123)
//!
//! A bare **accumulator** — `let sum = 0; for (…) sum = sum + x;` — still is
//! not enough. `benchmarks/suite/13_factorial.ts` is the live counterexample:
//! `sum = sum + (i % 1000)` over 1e8 iterations reaches 49,950,000,000,
//! twenty-three times `INT32_MAX`. Node prints it exactly; an i32 slot would
//! print a wrapped negative.
//!
//! The second half of this collector therefore proves a separate inequality.
//! A `for` loop whose update is a positive constant step on one of the
//! induction variables above has a compile-time trip-count upper bound `T`.
//! Bounds multiply through nested loops (with saturating arithmetic). For an
//! accumulator with literal entry value `A0`, every write must be
//! `acc = acc + e`, and `e` must have a small syntactic magnitude bound `M`:
//! an integer literal/constant, `x % literal`, `x & non_negative_literal`, or
//! another loop-bounded induction local. The accumulated proof is then
//! `|acc| <= |A0| + sum(T * M)`. Only a result at most `INT32_MAX` is admitted.
//! Unknown loop bounds and unknown expression shapes fall through to boxed
//! storage; under-approximation is free here.
//!
//! Consumed only by the canonical-i32 gate (`canonical_safe_local` in
//! `stmt/let_stmt.rs`), never by the parallel-shadow `needs_i32_slot` gate, so
//! `PERRY_CANONICAL_I32_LOCALS=0` still reproduces the pre-phase model
//! bit-for-bit — the same containment `int_valued_ta_locals` uses.

use std::collections::{HashMap, HashSet};

use perry_hir::{BinaryOp, CompareOp, Expr, LogicalOp, Stmt, UpdateOp};

/// Direction a local's induction moves in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dir {
    Inc,
    Dec,
}

/// The bound a single guarded loop level derives for one local.
#[derive(Clone, Copy, Debug)]
struct GuardedLevel {
    dir: Dir,
    /// The extreme the local can reach at this level: an upper bound for
    /// [`Dir::Inc`], a lower bound for [`Dir::Dec`]. Always inside i32.
    extreme: i64,
}

#[derive(Clone, Copy, Debug)]
struct IntInterval {
    lo: i64,
    hi: i64,
}

/// Analysis state, accumulated over one whole function body.
#[derive(Default)]
struct State {
    /// `id → I`, the single literal initialiser. Absent when the local has no
    /// declaration with an i32-range integer literal init.
    declared_init: HashMap<u32, i64>,
    /// Locals declared more than once, or whose declaration carries a
    /// non-literal / out-of-range init. Never admissible.
    bad_decl: HashSet<u32>,
    /// `id → value` for immutable integer-literal bindings never written
    /// anywhere. Used to resolve a symbolic loop bound (`i < LIMIT`).
    const_ints: HashMap<u32, i64>,
    /// Module-level `const NAME = <numeric literal>` bindings
    /// (`codegen/mod.rs`'s `compile_time_constants`). A `const` reassignment
    /// is a parse-time error in ECMAScript, so these are constants by the
    /// language rather than by analysis — the same map the Phase 2 in-bounds
    /// proof already trusts. Carries the `for (let i = 0; i < ITERATIONS; i++)`
    /// shape where `ITERATIONS` is declared at module scope.
    module_consts: HashMap<u32, i64>,
    /// A write that was not a guarded step. Never admissible.
    disqualified: HashSet<u32>,
    /// Merged bound across every guarded loop level that stepped this local.
    bounds: HashMap<u32, GuardedLevel>,
}

/// Locals proven to stay inside i32 range by the monotone loop-induction
/// argument documented at the top of this module.
pub fn collect_loop_bounded_i32_locals(
    stmts: &[Stmt],
    compile_time_constants: &HashMap<u32, f64>,
) -> HashSet<u32> {
    let mut st = State::default();
    st.module_consts = compile_time_constants
        .iter()
        .filter_map(|(&id, &v)| {
            (v.is_finite() && v.fract() == 0.0 && v.abs() <= i32::MAX as f64)
                .then_some((id, v as i64))
        })
        .collect();
    collect_declarations(stmts, &mut st);
    collect_const_ints(stmts, &mut st);

    let empty: HashMap<u32, GuardedLevel> = HashMap::new();
    walk_stmts(stmts, &empty, &mut st);

    let mut out = HashSet::new();
    for (&id, bound) in &st.bounds {
        if st.disqualified.contains(&id) || st.bad_decl.contains(&id) {
            continue;
        }
        let Some(&init) = st.declared_init.get(&id) else {
            continue;
        };
        // The interval is [min(init, extreme), max(init, extreme)] — for `Inc`
        // the local never goes below `init` and never above `extreme`; for
        // `Dec` the mirror. Both endpoints must fit i32; `extreme` already
        // does by construction, and `init` was range-checked at declaration,
        // but assert it here so the admission rule reads as the interval it is.
        let (lo, hi) = match bound.dir {
            Dir::Inc => (init, bound.extreme),
            Dir::Dec => (bound.extreme, init),
        };
        if fits_i32(lo) && fits_i32(hi) {
            out.insert(id);
        }
    }
    let induction_intervals = out
        .iter()
        .filter_map(|id| {
            let init = *st.declared_init.get(id)?;
            let bound = *st.bounds.get(id)?;
            let interval = match bound.dir {
                Dir::Inc => IntInterval {
                    lo: init,
                    hi: bound.extreme,
                },
                Dir::Dec => IntInterval {
                    lo: bound.extreme,
                    hi: init,
                },
            };
            Some((*id, interval))
        })
        .collect();
    out.extend(collect_bounded_accumulator_locals(
        stmts,
        &st,
        &induction_intervals,
        AccumulatorMode::I32Storage,
    ));
    out
}

/// #9363: accumulators whose `acc = acc + <byte read>` chain provably stays
/// below 2^53, so the update's `fadd` may carry `reassoc`.
///
/// Same proof, same trip-count machinery, weaker conclusion — see
/// [`AccumulatorMode`]. Consumed only by the emitter that adds the flag; it
/// changes no storage decision, so a wrong `true` cannot produce a wrapped or
/// mistyped value, only a differently-grouped sum (which the bound proves is
/// bit-identical anyway).
pub fn collect_reassociable_f64_accumulators(
    stmts: &[Stmt],
    compile_time_constants: &HashMap<u32, f64>,
) -> HashSet<u32> {
    let mut st = State::default();
    st.module_consts = compile_time_constants
        .iter()
        .filter_map(|(&id, &v)| {
            (v.is_finite() && v.fract() == 0.0 && v.abs() <= i32::MAX as f64)
                .then_some((id, v as i64))
        })
        .collect();
    collect_declarations(stmts, &mut st);
    collect_const_ints(stmts, &mut st);
    let empty: HashMap<u32, GuardedLevel> = HashMap::new();
    walk_stmts(stmts, &empty, &mut st);

    let induction_intervals: HashMap<u32, IntInterval> = st
        .bounds
        .iter()
        .filter_map(|(&id, bound)| {
            if st.disqualified.contains(&id) || st.bad_decl.contains(&id) {
                return None;
            }
            let init = *st.declared_init.get(&id)?;
            let interval = match bound.dir {
                Dir::Inc => IntInterval {
                    lo: init,
                    hi: bound.extreme,
                },
                Dir::Dec => IntInterval {
                    lo: bound.extreme,
                    hi: init,
                },
            };
            (fits_i32(interval.lo) && fits_i32(interval.hi)).then_some((id, interval))
        })
        .collect();

    collect_bounded_accumulator_locals(
        stmts,
        &st,
        &induction_intervals,
        AccumulatorMode::ReassocF64,
    )
}

fn fits_i32(n: i64) -> bool {
    i32::try_from(n).is_ok()
}

/// An integer-literal expression, if `e` is one (`Expr::Number` included so a
/// bound written `1e6` is not silently a different rule from `1000000`).
fn integer_literal(e: &Expr) -> Option<i64> {
    match e {
        Expr::Integer(n) => Some(*n),
        Expr::Number(f)
            if f.is_finite() && f.fract() == 0.0 && f.abs() <= 9.007_199_254_740_992e15 =>
        {
            Some(*f as i64)
        }
        _ => None,
    }
}

/// An i32-range integer constant: a literal, or a `const` local bound to one.
fn const_i32_value(e: &Expr, st: &State) -> Option<i64> {
    let v = match e {
        Expr::LocalGet(id) => *st.const_ints.get(id).or_else(|| st.module_consts.get(id))?,
        _ => integer_literal(e)?,
    };
    fits_i32(v).then_some(v)
}

// ---------------------------------------------------------------------------
// Pass 1: declarations and constants
// ---------------------------------------------------------------------------

/// Record each local's single literal initialiser. A local declared twice (a
/// hoisted `var`, a `let` in two switch arms sharing an id) loses the
/// single-entry-value premise the interval rests on, so it is rejected
/// outright.
fn collect_declarations(stmts: &[Stmt], st: &mut State) {
    for_each_let(stmts, &mut |id, init, _mutable| {
        if st.declared_init.contains_key(&id) {
            st.bad_decl.insert(id);
            return;
        }
        match init.and_then(integer_literal) {
            Some(v) if fits_i32(v) => {
                st.declared_init.insert(id, v);
            }
            _ => {
                st.bad_decl.insert(id);
            }
        }
    });
}

/// Immutable integer-literal bindings that are never written. These are the
/// only symbolic loop bounds this analysis will resolve (`const LIMIT = 1000`).
fn collect_const_ints(stmts: &[Stmt], st: &mut State) {
    let mut candidates: HashMap<u32, i64> = HashMap::new();
    for_each_let(stmts, &mut |id, init, mutable| {
        if mutable {
            return;
        }
        if let Some(v) = init.and_then(integer_literal) {
            if fits_i32(v) {
                candidates.insert(id, v);
            }
        }
    });
    if candidates.is_empty() {
        return;
    }
    let mut written: HashSet<u32> = HashSet::new();
    collect_written_locals(stmts, &mut written);
    candidates.retain(|id, _| !written.contains(id));
    st.const_ints = candidates;
}

/// Every local that is assigned anywhere in the body — `LocalSet` *and*
/// `Update`. `collect_localset_ids_in_stmts` deliberately skips `Update`
/// (it preserves integer-ness, which is what that collector asks about);
/// here a `++` is exactly what must disqualify a "constant" bound.
fn collect_written_locals(stmts: &[Stmt], out: &mut HashSet<u32>) {
    fn in_expr(e: &Expr, out: &mut HashSet<u32>) {
        match e {
            Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                out.insert(*id);
            }
            Expr::Closure { body, .. } => walk(body, out),
            _ => {
                if let Some(id) = with_set_fallback_local(e) {
                    out.insert(id);
                }
            }
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| in_expr(c, out));
    }
    fn walk(stmts: &[Stmt], out: &mut HashSet<u32>) {
        for s in stmts {
            match s {
                Stmt::Let { init, .. } => {
                    if let Some(e) = init {
                        in_expr(e, out);
                    }
                }
                Stmt::Expr(e) | Stmt::Throw(e) => in_expr(e, out),
                Stmt::Return(opt) => {
                    if let Some(e) = opt {
                        in_expr(e, out);
                    }
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    in_expr(condition, out);
                    walk(then_branch, out);
                    if let Some(eb) = else_branch {
                        walk(eb, out);
                    }
                }
                Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                    in_expr(condition, out);
                    walk(body, out);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(i) = init {
                        walk(std::slice::from_ref(i.as_ref()), out);
                    }
                    if let Some(c) = condition {
                        in_expr(c, out);
                    }
                    if let Some(u) = update {
                        in_expr(u, out);
                    }
                    walk(body, out);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    walk(body, out);
                    if let Some(c) = catch {
                        walk(&c.body, out);
                    }
                    if let Some(f) = finally {
                        walk(f, out);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    in_expr(discriminant, out);
                    for c in cases {
                        if let Some(t) = &c.test {
                            in_expr(t, out);
                        }
                        walk(&c.body, out);
                    }
                }
                Stmt::Labeled { body, .. } => walk(std::slice::from_ref(body.as_ref()), out),
                _ => {}
            }
        }
    }
    walk(stmts, out);
}

/// Visit every `Stmt::Let` in the body, including inside closures — a closure's
/// own `let` shares the id space, and missing it would let a second
/// declaration go unnoticed.
fn for_each_let(stmts: &[Stmt], f: &mut dyn FnMut(u32, Option<&Expr>, bool)) {
    for s in stmts {
        match s {
            Stmt::Let {
                id, init, mutable, ..
            } => {
                f(*id, init.as_ref(), *mutable);
                if let Some(init) = init {
                    for_each_let_in_expr(init, f);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => for_each_let_in_expr(e, f),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    for_each_let_in_expr(e, f);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                for_each_let_in_expr(condition, f);
                for_each_let(then_branch, f);
                if let Some(eb) = else_branch {
                    for_each_let(eb, f);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                for_each_let_in_expr(condition, f);
                for_each_let(body, f);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    for_each_let(std::slice::from_ref(init.as_ref()), f);
                }
                if let Some(c) = condition {
                    for_each_let_in_expr(c, f);
                }
                if let Some(u) = update {
                    for_each_let_in_expr(u, f);
                }
                for_each_let(body, f);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for_each_let(body, f);
                if let Some(c) = catch {
                    for_each_let(&c.body, f);
                }
                if let Some(fin) = finally {
                    for_each_let(fin, f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                for_each_let_in_expr(discriminant, f);
                for c in cases {
                    if let Some(t) = &c.test {
                        for_each_let_in_expr(t, f);
                    }
                    for_each_let(&c.body, f);
                }
            }
            Stmt::Labeled { body, .. } => {
                for_each_let(std::slice::from_ref(body.as_ref()), f);
            }
            _ => {}
        }
    }
}

fn for_each_let_in_expr(e: &Expr, f: &mut dyn FnMut(u32, Option<&Expr>, bool)) {
    if let Expr::Closure { body, .. } = e {
        for_each_let(body, f);
    }
    perry_hir::walker::walk_expr_children(e, &mut |child| for_each_let_in_expr(child, f));
}

// ---------------------------------------------------------------------------
// Pass 2: judge every write against the guard of its immediately-enclosing loop
// ---------------------------------------------------------------------------

/// A step write: `(direction, magnitude)`.
fn classify_step(id: u32, value: &Expr, st: &State) -> Option<(Dir, i64)> {
    let Expr::Binary { op, left, right } = value else {
        return None;
    };
    let dir = match op {
        BinaryOp::Add => Dir::Inc,
        BinaryOp::Sub => Dir::Dec,
        _ => return None,
    };
    // `v = v + k` (and, for Add only, the commuted `v = k + v`). `v = k - v`
    // is NOT a step: it flips sign every iteration and is not monotone.
    let is_self = |e: &Expr| matches!(e, Expr::LocalGet(other) if *other == id);
    let k = if is_self(left) {
        const_i32_value(right, st)?
    } else if matches!(dir, Dir::Inc) && is_self(right) {
        const_i32_value(left, st)?
    } else {
        return None;
    };
    // A negative magnitude reverses the direction and would break monotonicity
    // against the guard; reject rather than normalise, so the rule stays the
    // one the module doc states.
    (k >= 0).then_some((dir, k))
}

/// Every step site for `id` at ONE loop level: the loop's `update` plus its
/// body, descending through `if`/`switch`/`try`/labels but never into a nested
/// loop or a closure. Returns `None` when any write at this level is not a
/// step, when the directions disagree, or when there is no step at all.
fn scan_level_steps(
    update: Option<&Expr>,
    body: &[Stmt],
    id: u32,
    st: &State,
) -> Option<(Dir, i64)> {
    let mut acc: Option<(Dir, i64)> = None;
    let mut ok = true;
    let mut fold = |step: Option<(Dir, i64)>| match (step, acc) {
        (None, _) => ok = false,
        (Some((d, k)), None) => acc = Some((d, k)),
        (Some((d, k)), Some((d0, total))) => {
            if d == d0 {
                acc = Some((d0, total.saturating_add(k)));
            } else {
                ok = false;
            }
        }
    };
    if let Some(u) = update {
        scan_level_writes_in_expr(u, id, st, &mut fold);
    }
    scan_level_writes(body, id, st, &mut fold);
    if !ok {
        return None;
    }
    acc
}

fn scan_level_writes(stmts: &[Stmt], id: u32, st: &State, f: &mut dyn FnMut(Option<(Dir, i64)>)) {
    for s in stmts {
        match s {
            Stmt::Let { init, .. } => {
                if let Some(init) = init {
                    scan_level_writes_in_expr(init, id, st, f);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => scan_level_writes_in_expr(e, id, st, f),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    scan_level_writes_in_expr(e, id, st, f);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                scan_level_writes_in_expr(condition, id, st, f);
                scan_level_writes(then_branch, id, st, f);
                if let Some(eb) = else_branch {
                    scan_level_writes(eb, id, st, f);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                scan_level_writes(body, id, st, f);
                if let Some(c) = catch {
                    scan_level_writes(&c.body, id, st, f);
                }
                if let Some(fin) = finally {
                    scan_level_writes(fin, id, st, f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                scan_level_writes_in_expr(discriminant, id, st, f);
                for c in cases {
                    if let Some(t) = &c.test {
                        scan_level_writes_in_expr(t, id, st, f);
                    }
                    scan_level_writes(&c.body, id, st, f);
                }
            }
            Stmt::Labeled { body, .. } => {
                scan_level_writes(std::slice::from_ref(body.as_ref()), id, st, f);
            }
            // A nested loop is a different level: its writes are judged against
            // its OWN guard by `walk_stmts`, and must not be folded into this
            // level's per-iteration total (they can execute many times per
            // outer iteration).
            Stmt::While { .. } | Stmt::DoWhile { .. } | Stmt::For { .. } => {}
            _ => {}
        }
    }
}

fn scan_level_writes_in_expr(e: &Expr, id: u32, st: &State, f: &mut dyn FnMut(Option<(Dir, i64)>)) {
    match e {
        Expr::LocalSet(target, value) => {
            if *target == id {
                f(classify_step(id, value, st));
            }
            scan_level_writes_in_expr(value, id, st, f);
        }
        Expr::Update { id: target, op, .. } => {
            if *target == id {
                f(Some((update_dir(*op), 1)));
            }
        }
        // A closure body is not part of this level (and any write it makes is
        // disqualifying — `walk_stmts` judges it against an empty guard).
        Expr::Closure { .. } => {}
        _ => {
            if with_set_fallback_local(e) == Some(id) {
                f(None);
            }
            perry_hir::walker::walk_expr_children(e, &mut |child| {
                scan_level_writes_in_expr(child, id, st, f)
            });
        }
    }
}

/// The local a `with (obj) { name = v }` write falls back to when the object
/// does not bind `name`. That write never appears as a `LocalSet`, so without
/// this arm a `with` block could assign a counter behind the analysis's back.
fn with_set_fallback_local(e: &Expr) -> Option<u32> {
    let Expr::WithSet { fallback, .. } = e else {
        return None;
    };
    match fallback {
        perry_hir::WithSetFallback::Local(id) | perry_hir::WithSetFallback::SloppyImplicit(id) => {
            Some(*id)
        }
        _ => None,
    }
}

fn update_dir(op: UpdateOp) -> Dir {
    match op {
        UpdateOp::Increment => Dir::Inc,
        UpdateOp::Decrement => Dir::Dec,
    }
}

/// The guards a loop's condition establishes for the locals it steps.
fn derive_guards(
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
    st: &State,
) -> HashMap<u32, GuardedLevel> {
    let mut out = HashMap::new();
    let Some(condition) = condition else {
        return out;
    };
    for (id, op, bound) in conjunct_guards(condition, st) {
        let Some((dir, total)) = scan_level_steps(update, body, id, st) else {
            continue;
        };
        let guard_dir = match op {
            CompareOp::Lt | CompareOp::Le => Dir::Inc,
            CompareOp::Gt | CompareOp::Ge => Dir::Dec,
            _ => continue,
        };
        if dir != guard_dir {
            continue;
        }
        // The guard is observed true immediately before the iteration that
        // contains the steps, so the value entering them is at most `bound - 1`
        // (`bound` for the inclusive forms) and the steps add at most `total`.
        let extreme = match op {
            CompareOp::Lt => bound - 1 + total,
            CompareOp::Le => bound + total,
            CompareOp::Gt => bound + 1 - total,
            CompareOp::Ge => bound - total,
            _ => continue,
        };
        if !fits_i32(extreme) {
            continue;
        }
        // Two conjuncts can name the same local (`i < A && i < B`); either is a
        // valid bound, so keep the tighter one.
        match out.entry(id) {
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(GuardedLevel { dir, extreme });
            }
            std::collections::hash_map::Entry::Occupied(mut o) => {
                let cur: &mut GuardedLevel = o.get_mut();
                if cur.dir == dir {
                    cur.extreme = match dir {
                        Dir::Inc => cur.extreme.min(extreme),
                        Dir::Dec => cur.extreme.max(extreme),
                    };
                }
            }
        }
    }
    out
}

/// Every `local OP const` comparison on the condition's top-level `&&` spine.
/// A conjunct is guaranteed true whenever the body runs; `||` and `!` give no
/// such guarantee and are not descended into.
fn conjunct_guards(e: &Expr, st: &State) -> Vec<(u32, CompareOp, i64)> {
    let mut out = Vec::new();
    collect_conjunct_guards(e, st, &mut out);
    out
}

fn collect_conjunct_guards(e: &Expr, st: &State, out: &mut Vec<(u32, CompareOp, i64)>) {
    match e {
        Expr::Logical {
            op: LogicalOp::And,
            left,
            right,
        } => {
            collect_conjunct_guards(left, st, out);
            collect_conjunct_guards(right, st, out);
        }
        Expr::Compare { op, left, right } => {
            if let (Expr::LocalGet(id), Some(bound)) = (left.as_ref(), const_i32_value(right, st)) {
                out.push((*id, *op, bound));
            } else if let (Some(bound), Expr::LocalGet(id)) =
                (const_i32_value(left, st), right.as_ref())
            {
                // `B > i` is `i < B`, etc.
                if let Some(flipped) = flip_compare(*op) {
                    out.push((*id, flipped, bound));
                }
            }
        }
        _ => {}
    }
}

fn flip_compare(op: CompareOp) -> Option<CompareOp> {
    match op {
        CompareOp::Lt => Some(CompareOp::Gt),
        CompareOp::Le => Some(CompareOp::Ge),
        CompareOp::Gt => Some(CompareOp::Lt),
        CompareOp::Ge => Some(CompareOp::Le),
        _ => None,
    }
}

fn walk_stmts(stmts: &[Stmt], guard: &HashMap<u32, GuardedLevel>, st: &mut State) {
    for s in stmts {
        match s {
            Stmt::Let { init, .. } => {
                if let Some(init) = init {
                    walk_expr(init, guard, st);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => walk_expr(e, guard, st),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    walk_expr(e, guard, st);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                walk_expr(condition, guard, st);
                walk_stmts(then_branch, guard, st);
                if let Some(eb) = else_branch {
                    walk_stmts(eb, guard, st);
                }
            }
            Stmt::While { condition, body } => {
                // The condition is evaluated before each iteration, so a write
                // INSIDE it is not covered by the guard it is establishing.
                walk_expr(condition, guard, st);
                let level = derive_guards(Some(condition), None, body, st);
                walk_stmts(body, &level, st);
            }
            // `do { … } while (c)` runs its body once BEFORE `c` is ever
            // evaluated, so no guard dominates the first pass. Judged at the
            // enclosing level, which disqualifies any step it contains.
            Stmt::DoWhile { body, condition } => {
                walk_stmts(body, &HashMap::new(), st);
                walk_expr(condition, guard, st);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    walk_stmts(std::slice::from_ref(init.as_ref()), guard, st);
                }
                if let Some(c) = condition {
                    walk_expr(c, guard, st);
                }
                let level = derive_guards(condition.as_ref(), update.as_ref(), body, st);
                if let Some(u) = update {
                    walk_expr(u, &level, st);
                }
                walk_stmts(body, &level, st);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                walk_stmts(body, guard, st);
                if let Some(c) = catch {
                    walk_stmts(&c.body, guard, st);
                }
                if let Some(fin) = finally {
                    walk_stmts(fin, guard, st);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                walk_expr(discriminant, guard, st);
                for c in cases {
                    if let Some(t) = &c.test {
                        walk_expr(t, guard, st);
                    }
                    walk_stmts(&c.body, guard, st);
                }
            }
            Stmt::Labeled { body, .. } => {
                walk_stmts(std::slice::from_ref(body.as_ref()), guard, st);
            }
            _ => {}
        }
    }
}

fn walk_expr(e: &Expr, guard: &HashMap<u32, GuardedLevel>, st: &mut State) {
    match e {
        Expr::LocalSet(id, value) => {
            let step = classify_step(*id, value, st);
            record_write(*id, step, guard, st);
            walk_expr(value, guard, st);
        }
        Expr::Update { id, op, .. } => {
            record_write(*id, Some((update_dir(*op), 1)), guard, st);
        }
        Expr::Closure { body, .. } => {
            // No loop guard reaches into a closure body: it can be called any
            // number of times, from anywhere. Any write it makes disqualifies.
            let empty = HashMap::new();
            walk_stmts(body, &empty, st);
            // Param defaults are ordinary expressions evaluated per call.
            perry_hir::walker::walk_expr_children(e, &mut |child| walk_expr(child, &empty, st));
        }
        _ => {
            // A `with (obj) { i = v }` write is not a `LocalSet`; its target
            // lives in the `WithSet` fallback and is unconditionally
            // disqualifying (the object probe decides at runtime whether the
            // local is written at all).
            if let Some(id) = with_set_fallback_local(e) {
                st.disqualified.insert(id);
            }
            // `walk_expr_children` is the crate's single source of truth for
            // sub-expression descent, so a future variant carrying a
            // `LocalSet`/`Update` is judged rather than silently skipped.
            perry_hir::walker::walk_expr_children(e, &mut |child| walk_expr(child, guard, st));
        }
    }
}

fn record_write(
    id: u32,
    step: Option<(Dir, i64)>,
    guard: &HashMap<u32, GuardedLevel>,
    st: &mut State,
) {
    let Some((dir, _)) = step else {
        st.disqualified.insert(id);
        return;
    };
    let Some(level) = guard.get(&id).copied() else {
        st.disqualified.insert(id);
        return;
    };
    if level.dir != dir {
        st.disqualified.insert(id);
        return;
    }
    match st.bounds.entry(id) {
        std::collections::hash_map::Entry::Vacant(v) => {
            v.insert(level);
        }
        std::collections::hash_map::Entry::Occupied(mut o) => {
            let cur: &mut GuardedLevel = o.get_mut();
            if cur.dir != level.dir {
                st.disqualified.insert(id);
                return;
            }
            // Widen to the union of every guarded level that steps this local.
            cur.extreme = match level.dir {
                Dir::Inc => cur.extreme.max(level.extreme),
                Dir::Dec => cur.extreme.min(level.extreme),
            };
        }
    }
}

// ---------------------------------------------------------------------------
// Pass 3: trip-count x step-magnitude bounds for accumulators (#7123)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct ExecutionBound {
    loop_depth: u32,
    executions: Option<u128>,
}

impl ExecutionBound {
    fn function_body() -> Self {
        Self {
            loop_depth: 0,
            executions: Some(1),
        }
    }

    fn nested_loop(self, trips: Option<u128>) -> Self {
        Self {
            loop_depth: self.loop_depth.saturating_add(1),
            executions: self
                .executions
                .zip(trips)
                .map(|(outer, inner)| outer.saturating_mul(inner)),
        }
    }
}

/// Which admission the accumulator pass is computing.
///
/// Both share one proof — `|acc| <= |A0| + sum(T * M)` over the trip-count
/// machinery above — and differ only in what magnitudes count as bounded and
/// how large the result may be.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AccumulatorMode {
    /// #7123: admit to canonical i32 STORAGE. Every addend must be an
    /// integer-valued expression, because the slot cannot hold anything else.
    I32Storage,
    /// #9363: admit only to `fadd reassoc` on the accumulator update — the
    /// value stays an f64 in the same slot it always used.
    ///
    /// This is a strictly weaker claim, so it admits strictly more:
    ///
    ///  * a byte read (`buf[i]`) counts with magnitude 255. It is NOT
    ///    integer-valued in general — an out-of-range read is `undefined`,
    ///    i.e. NaN in arithmetic — which is exactly why `I32Storage` must
    ///    refuse it: an i32 slot cannot represent NaN. Reassociation does not
    ///    care: NaN propagates through every grouping alike, so a chain
    ///    containing one is NaN under any association, and a chain without one
    ///    is a sum of exact integers.
    ///  * the limit is 2^53 rather than `i32::MAX`, because the claim is only
    ///    that every partial sum is exactly representable as an f64 (where
    ///    addition is associative), not that it fits an integer register.
    ReassocF64,
}

impl AccumulatorMode {
    fn limit(self) -> u128 {
        match self {
            // Below 2^53 every integer is exactly representable, so f64
            // addition is exact and therefore associative: any grouping of the
            // same addends yields bit-identical results.
            Self::ReassocF64 => 1u128 << 53,
            Self::I32Storage => i32::MAX as u128,
        }
    }
}

#[derive(Default)]
struct AccumulatorState {
    /// Sum of the worst-case magnitude contribution from every syntactic write
    /// site. Mutually-exclusive sites are deliberately summed: overestimating
    /// only declines an optimization, while underestimating would wrap values.
    contribution: HashMap<u32, u128>,
    saw_accumulation: HashSet<u32>,
    disqualified: HashSet<u32>,
}

fn collect_bounded_accumulator_locals(
    stmts: &[Stmt],
    induction: &State,
    induction_intervals: &HashMap<u32, IntInterval>,
    mode: AccumulatorMode,
) -> HashSet<u32> {
    let mut accumulators = AccumulatorState::default();
    walk_accumulator_stmts(
        stmts,
        ExecutionBound::function_body(),
        induction,
        induction_intervals,
        mode,
        &mut accumulators,
    );

    accumulators
        .saw_accumulation
        .iter()
        .filter_map(|&id| {
            if accumulators.disqualified.contains(&id) || induction.bad_decl.contains(&id) {
                return None;
            }
            let init = *induction.declared_init.get(&id)?;
            let contribution = *accumulators.contribution.get(&id)?;
            let worst_magnitude = u128::from(init.unsigned_abs()).saturating_add(contribution);
            (worst_magnitude <= mode.limit()).then_some(id)
        })
        .collect()
}

/// A direct, guaranteed `for` update. Restricting the trip-count premise to a
/// single update expression avoids treating a short-circuited or conditional
/// body step as minimum progress. Extra same-direction body steps can only
/// reduce the number of iterations; the induction pass has already rejected
/// any reversing or otherwise-unrecognised write to this counter.
fn direct_for_update(update: &Expr, st: &State) -> Option<(u32, Dir, u64)> {
    let (id, dir, magnitude) = match update {
        Expr::Update { id, op, .. } => (*id, update_dir(*op), 1),
        Expr::LocalSet(id, value) => {
            let (dir, magnitude) = classify_step(*id, value, st)?;
            (*id, dir, magnitude)
        }
        _ => return None,
    };
    let magnitude = u64::try_from(magnitude).ok()?;
    (magnitude > 0).then_some((id, dir, magnitude))
}

fn for_loop_trip_bound(
    condition: Option<&Expr>,
    update: Option<&Expr>,
    st: &State,
    induction_intervals: &HashMap<u32, IntInterval>,
) -> Option<u128> {
    let condition = condition?;
    let (id, dir, step) = direct_for_update(update?, st)?;
    if !induction_intervals.contains_key(&id) {
        return None;
    }
    let init = *st.declared_init.get(&id)?;
    conjunct_guards(condition, st)
        .into_iter()
        .filter(|(guarded, _, _)| *guarded == id)
        .filter_map(|(_, op, bound)| trip_count_for_guard(init, dir, step, op, bound))
        .min()
}

fn trip_count_for_guard(init: i64, dir: Dir, step: u64, op: CompareOp, bound: i64) -> Option<u128> {
    let (distance, agrees) = match (dir, op) {
        (Dir::Inc, CompareOp::Lt) => ((i128::from(bound) - i128::from(init)).max(0), true),
        (Dir::Inc, CompareOp::Le) => ((i128::from(bound) - i128::from(init) + 1).max(0), true),
        (Dir::Dec, CompareOp::Gt) => ((i128::from(init) - i128::from(bound)).max(0), true),
        (Dir::Dec, CompareOp::Ge) => ((i128::from(init) - i128::from(bound) + 1).max(0), true),
        _ => (0, false),
    };
    if !agrees {
        return None;
    }
    let distance = u128::try_from(distance).ok()?;
    let step = u128::from(step);
    Some(distance.saturating_add(step - 1) / step)
}

fn accumulator_step_magnitude(
    id: u32,
    value: &Expr,
    st: &State,
    induction_intervals: &HashMap<u32, IntInterval>,
    mode: AccumulatorMode,
) -> Option<u128> {
    let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = value
    else {
        return None;
    };
    let is_self = |e: &Expr| matches!(e, Expr::LocalGet(other) if *other == id);
    let step = if is_self(left) {
        right.as_ref()
    } else if is_self(right) {
        left.as_ref()
    } else {
        return None;
    };
    step_magnitude_bound(step, st, induction_intervals, mode)
}

fn step_magnitude_bound(
    e: &Expr,
    st: &State,
    induction_intervals: &HashMap<u32, IntInterval>,
    mode: AccumulatorMode,
) -> Option<u128> {
    // A `Uint8Array` / `Buffer` element is a byte, so its magnitude is at most
    // 255 — the tightest bound any addend in this analysis has. It is admitted
    // ONLY under `ReassocF64`: an out-of-range read is `undefined`, hence NaN
    // in arithmetic, which is not an integer value and must never reach an i32
    // slot. Reassociation tolerates it because NaN propagates identically
    // through every grouping. See `AccumulatorMode`.
    if mode == AccumulatorMode::ReassocF64
        && matches!(e, Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. })
    {
        return Some(255);
    }
    if let Some(value) = integer_literal(e) {
        return Some(u128::from(value.unsigned_abs()));
    }
    if let Expr::LocalGet(id) = e {
        if let Some(value) = st.const_ints.get(id).or_else(|| st.module_consts.get(id)) {
            return Some(u128::from(value.unsigned_abs()));
        }
        let interval = induction_intervals.get(id)?;
        return Some(
            u128::from(interval.lo.unsigned_abs()).max(u128::from(interval.hi.unsigned_abs())),
        );
    }
    let Expr::Binary { op, left, right } = e else {
        return None;
    };
    match op {
        // JS remainder has the sign of its dividend and magnitude strictly
        // below the divisor's magnitude. The dividend must itself be an
        // integer-proven value; otherwise `1.5 % 1` is fractional and a range
        // bound alone would not justify integer storage. Zero is excluded
        // because `% 0` is NaN, not an integer step.
        BinaryOp::Mod => {
            let dividend_is_integer = integer_literal(left).is_some()
                || matches!(left.as_ref(), Expr::LocalGet(id)
                    if st.const_ints.contains_key(id)
                        || st.module_consts.contains_key(id)
                        || induction_intervals.contains_key(id));
            if !dividend_is_integer {
                return None;
            }
            let divisor = integer_literal(right)?;
            let magnitude = u128::from(divisor.unsigned_abs());
            (magnitude > 0).then(|| magnitude - 1)
        }
        // Bitwise operands are ToInt32-coerced. For a non-negative literal
        // mask, the source literal itself is a conservative magnitude bound
        // even when it exceeds the signed-i32 range.
        BinaryOp::BitAnd => {
            let mask = integer_literal(right)
                .or_else(|| integer_literal(left))
                .filter(|mask| *mask >= 0)?;
            Some(mask as u128)
        }
        _ => None,
    }
}

fn record_accumulator_write(
    id: u32,
    value: Option<&Expr>,
    execution: ExecutionBound,
    induction: &State,
    induction_intervals: &HashMap<u32, IntInterval>,
    mode: AccumulatorMode,
    accumulators: &mut AccumulatorState,
) {
    let magnitude = value.and_then(|value| {
        accumulator_step_magnitude(id, value, induction, induction_intervals, mode)
    });
    let Some((executions, magnitude)) = execution.executions.zip(magnitude) else {
        accumulators.disqualified.insert(id);
        return;
    };
    if execution.loop_depth == 0 {
        accumulators.disqualified.insert(id);
        return;
    }
    accumulators.saw_accumulation.insert(id);
    let contribution = executions.saturating_mul(magnitude);
    accumulators
        .contribution
        .entry(id)
        .and_modify(|total| *total = total.saturating_add(contribution))
        .or_insert(contribution);
}

fn walk_accumulator_stmts(
    stmts: &[Stmt],
    execution: ExecutionBound,
    induction: &State,
    induction_intervals: &HashMap<u32, IntInterval>,
    mode: AccumulatorMode,
    accumulators: &mut AccumulatorState,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(init) = init {
                    walk_accumulator_expr(
                        init,
                        execution,
                        induction,
                        induction_intervals,
                        mode,
                        accumulators,
                    );
                }
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) => walk_accumulator_expr(
                expr,
                execution,
                induction,
                induction_intervals,
                mode,
                accumulators,
            ),
            Stmt::Return(value) => {
                if let Some(value) = value {
                    walk_accumulator_expr(
                        value,
                        execution,
                        induction,
                        induction_intervals,
                        mode,
                        accumulators,
                    );
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                walk_accumulator_expr(
                    condition,
                    execution,
                    induction,
                    induction_intervals,
                    mode,
                    accumulators,
                );
                walk_accumulator_stmts(
                    then_branch,
                    execution,
                    induction,
                    induction_intervals,
                    mode,
                    accumulators,
                );
                if let Some(else_branch) = else_branch {
                    walk_accumulator_stmts(
                        else_branch,
                        execution,
                        induction,
                        induction_intervals,
                        mode,
                        accumulators,
                    );
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                // This first implementation intentionally derives execution
                // counts only from a `for` loop's guaranteed update. A while
                // body step may be skipped by control flow; treating it as
                // minimum progress would make the trip bound unsound.
                let unknown = execution.nested_loop(None);
                walk_accumulator_expr(
                    condition,
                    unknown,
                    induction,
                    induction_intervals,
                    mode,
                    accumulators,
                );
                walk_accumulator_stmts(body, unknown, induction, induction_intervals, mode, accumulators);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    walk_accumulator_stmts(
                        std::slice::from_ref(init.as_ref()),
                        execution,
                        induction,
                        induction_intervals,
                        mode,
                        accumulators,
                    );
                }
                let trips = for_loop_trip_bound(
                    condition.as_ref(),
                    update.as_ref(),
                    induction,
                    induction_intervals,
                );
                let loop_execution = execution.nested_loop(trips);
                if let Some(condition) = condition {
                    // The condition executes once more than the body and may
                    // short-circuit. Writes there are outside this proof.
                    walk_accumulator_expr(
                        condition,
                        execution.nested_loop(None),
                        induction,
                        induction_intervals,
                        mode,
                        accumulators,
                    );
                }
                if let Some(update) = update {
                    walk_accumulator_expr(
                        update,
                        loop_execution,
                        induction,
                        induction_intervals,
                        mode,
                        accumulators,
                    );
                }
                walk_accumulator_stmts(
                    body,
                    loop_execution,
                    induction,
                    induction_intervals,
                    mode,
                    accumulators,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                walk_accumulator_stmts(
                    body,
                    execution,
                    induction,
                    induction_intervals,
                    mode,
                    accumulators,
                );
                if let Some(catch) = catch {
                    walk_accumulator_stmts(
                        &catch.body,
                        execution,
                        induction,
                        induction_intervals,
                        mode,
                        accumulators,
                    );
                }
                if let Some(finally) = finally {
                    walk_accumulator_stmts(
                        finally,
                        execution,
                        induction,
                        induction_intervals,
                        mode,
                        accumulators,
                    );
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                walk_accumulator_expr(
                    discriminant,
                    execution,
                    induction,
                    induction_intervals,
                    mode,
                    accumulators,
                );
                for case in cases {
                    if let Some(test) = &case.test {
                        walk_accumulator_expr(
                            test,
                            execution,
                            induction,
                            induction_intervals,
                            mode,
                            accumulators,
                        );
                    }
                    walk_accumulator_stmts(
                        &case.body,
                        execution,
                        induction,
                        induction_intervals,
                        mode,
                        accumulators,
                    );
                }
            }
            Stmt::Labeled { body, .. } => walk_accumulator_stmts(
                std::slice::from_ref(body.as_ref()),
                execution,
                induction,
                induction_intervals,
                mode,
                accumulators,
            ),
            _ => {}
        }
    }
}

fn walk_accumulator_expr(
    expr: &Expr,
    execution: ExecutionBound,
    induction: &State,
    induction_intervals: &HashMap<u32, IntInterval>,
    mode: AccumulatorMode,
    accumulators: &mut AccumulatorState,
) {
    match expr {
        Expr::LocalSet(id, value) => {
            record_accumulator_write(
                *id,
                Some(value),
                execution,
                induction,
                induction_intervals,
                mode,
                accumulators,
            );
            walk_accumulator_expr(
                value,
                execution,
                induction,
                induction_intervals,
                mode,
                accumulators,
            );
        }
        Expr::Update { id, .. } => record_accumulator_write(
            *id,
            None,
            execution,
            induction,
            induction_intervals,
            mode,
            accumulators,
        ),
        Expr::Closure { body, .. } => {
            // A closure can be called any number of times. Disqualify every
            // local it writes in this enclosing analysis; its own codegen pass
            // will independently analyse locals owned by the closure.
            let mut written = HashSet::new();
            collect_written_locals(body, &mut written);
            accumulators.disqualified.extend(written);
            let unknown = execution.nested_loop(None);
            perry_hir::walker::walk_expr_children(expr, &mut |child| {
                walk_accumulator_expr(child, unknown, induction, induction_intervals, mode, accumulators)
            });
        }
        _ => {
            if let Some(id) = with_set_fallback_local(expr) {
                accumulators.disqualified.insert(id);
            }
            perry_hir::walker::walk_expr_children(expr, &mut |child| {
                walk_accumulator_expr(
                    child,
                    execution,
                    induction,
                    induction_intervals,
                    mode,
                    accumulators,
                )
            });
        }
    }
}

#[cfg(test)]
#[path = "loop_bounded_i32/tests.rs"]
mod tests;
