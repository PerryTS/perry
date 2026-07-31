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
//! ## What it deliberately does NOT prove
//!
//! A bare **accumulator** — `let sum = 0; for (…) sum = sum + x;` — is not
//! admitted, and must not be. `benchmarks/suite/13_factorial.ts` is the live
//! counterexample: `sum = sum + (i % 1000)` over 1e8 iterations reaches
//! 49,950,000,000, twenty-three times `INT32_MAX`. Node prints it exactly; an
//! i32 slot would print a wrapped negative. Admitting "every write is
//! `sum = sum + <integer>`" as an i32 proof is a silent wrong answer, not a
//! missed optimization. Bounding an accumulator needs the loop's *trip count*
//! multiplied by a magnitude bound on the step expression — strictly more
//! analysis than this module does, and out of scope here.
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
    out
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

#[cfg(test)]
#[path = "loop_bounded_i32/tests.rs"]
mod tests;
