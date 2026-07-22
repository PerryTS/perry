//! Integer-local analysis: which locals are provably integer-valued, making
//! them eligible for an i32 shadow slot (see `needs_i32_slot` in
//! `stmt/let_stmt.rs`).
//!
//! ## Soundness invariant (regression #4785 and its bug class)
//!
//! A wrong i32 slot is a miscompile: the f64 slot holds a NaN-boxed pointer,
//! the i32 read does `fptosi` on the NaN and produces `i32::MIN`, and user
//! code crashes with `(number).method is not a function`. Losing a slot on a
//! rare pattern is only a missed optimization. The analysis is therefore
//! structured so that **no admission path — syntactic seed, forward
//! propagation, clamp-call admission, or flat-const admission — can escape
//! transitive disqualification**:
//!
//! 1. Admission runs optimistically to a fixed point
//!    (`collect_integer_let_ids` + `collect_extra_integer_let_ids`).
//! 2. Every candidate is then re-judged through ALL of its defining
//!    expressions — its `Let` init (the lone exemption is the optimistic
//!    `let x = undefined`-with-later-writes scaffolding seed, whose real
//!    values are its writes) and every `LocalSet` rhs targeting it. The
//!    judgment (`int32_producing_deps`) records *provenance*: the exact set
//!    of other locals whose candidate-ness the verdict relied on.
//! 3. Any failed judgment disqualifies the local, and disqualification
//!    propagates transitively through the recorded reverse-dependency edges
//!    with a worklist — any number of hops, regardless of how the dependent
//!    was admitted. Locals written inside closures are disqualified
//!    unconditionally.
//!
//! `int32_producing_deps` is deliberately stricter than the admission-side
//! `is_int32_producing_expr` in two places where the latter is optimistic:
//! `Expr::Update` requires the updated local to itself be a candidate, and a
//! call to an *argument-dependent* clamp function (`clamp3`-shaped functions
//! return one of their arguments verbatim) requires every argument to be
//! int-producing. Anything admission accepted that judgment rejects is simply
//! pruned.
//!
//! Scoping notes: `clamp_fn_ids` are *function* ids (module-global, no
//! per-function contamination). `flat_const_ids` are module-init local ids of
//! never-mutated `const` int matrices; those facts cannot change during this
//! per-function analysis, so flat-const admissions carry no local deps. (HIR
//! local ids are scope-local, so a function-local id can in principle collide
//! with a module-init flat-const id — that exposure is shared with codegen's
//! `flat_const_arrays` fast-path lowering and is out of scope here.)

use perry_hir::{BinaryOp, Expr, Stmt};
use std::collections::{HashMap, HashSet};

use super::*;

/// Element-value-fits-signed-i32 `TYPED_ARRAY_KIND_*` tags (excludes
/// `Uint32Array`, whose upper half does not round-trip through an i32 slot,
/// and the float/BigInt kinds).
fn typed_array_kind_elem_fits_i32(kind: u8) -> bool {
    use perry_hir::{
        TYPED_ARRAY_KIND_INT16, TYPED_ARRAY_KIND_INT32, TYPED_ARRAY_KIND_INT8,
        TYPED_ARRAY_KIND_UINT16, TYPED_ARRAY_KIND_UINT8, TYPED_ARRAY_KIND_UINT8_CLAMPED,
    };
    matches!(
        kind,
        TYPED_ARRAY_KIND_INT8
            | TYPED_ARRAY_KIND_UINT8
            | TYPED_ARRAY_KIND_UINT8_CLAMPED
            | TYPED_ARRAY_KIND_INT16
            | TYPED_ARRAY_KIND_UINT16
            | TYPED_ARRAY_KIND_INT32
    )
}

/// Context-free value window of an index expression: `Some((lo, hi))` proves
/// the JS value is an integer in `[lo, hi]` for EVERY runtime environment.
/// Only shapes whose result is integral by construction qualify — literals and
/// `ToInt32`/`ToUint32`-wrapping bitwise ops (`e & K` is `[0, K]` for any `e`,
/// `e >>> k` is bounded by the shift) — plus `+`/`-` compositions of such
/// windows. This is the syntactic sibling of the ctx-aware
/// `int_range_expr` rules in `expr/range_facts.rs`; collectors run before any
/// `FnCtx` exists, so they cannot consult local range facts.
pub(crate) fn static_index_window(e: &perry_hir::Expr) -> Option<(i64, i64)> {
    use perry_hir::{BinaryOp, Expr};
    fn int_constant(e: &Expr) -> Option<i64> {
        match e {
            Expr::Integer(n) => Some(*n),
            Expr::Number(n) if n.is_finite() && n.fract() == 0.0 && n.abs() < 2f64.powi(53) => {
                Some(*n as i64)
            }
            _ => None,
        }
    }
    fn ones_cover(value: i64) -> i64 {
        if value == 0 {
            0
        } else {
            ((1u64 << (64 - (value as u64).leading_zeros())) - 1) as i64
        }
    }
    if let Some(n) = int_constant(e) {
        return Some((n, n));
    }
    let Expr::Binary { op, left, right } = e else {
        return None;
    };
    match op {
        BinaryOp::BitAnd => {
            let mask = int_constant(left)
                .or_else(|| int_constant(right))
                .filter(|mask| (0..=i64::from(i32::MAX)).contains(mask))?;
            Some((0, mask))
        }
        BinaryOp::UShr => {
            let max = match int_constant(right).map(|k| (k as u64) & 31) {
                Some(k) if k > 0 => (1i64 << (32 - k)) - 1,
                _ => i64::from(u32::MAX),
            };
            Some((0, max))
        }
        BinaryOp::Add => {
            let (ll, lh) = static_index_window(left)?;
            let (rl, rh) = static_index_window(right)?;
            Some((ll.checked_add(rl)?, lh.checked_add(rh)?))
        }
        BinaryOp::Sub => {
            let (ll, lh) = static_index_window(left)?;
            let (rl, rh) = static_index_window(right)?;
            Some((ll.checked_sub(rh)?, lh.checked_sub(rl)?))
        }
        BinaryOp::BitOr => {
            let (ll, lh) = static_index_window(left)?;
            let (rl, rh) = static_index_window(right)?;
            if ll >= 0 && rl >= 0 && lh <= i64::from(i32::MAX) && rh <= i64::from(i32::MAX) {
                Some((ll.max(rl), ones_cover(lh) | ones_cover(rh)))
            } else {
                None
            }
        }
        BinaryOp::Shr => {
            let shift = int_constant(right).map(|k| (k as u64) & 31)?;
            if shift == 0 {
                return None;
            }
            Some((i64::from(i32::MIN) >> shift, i64::from(i32::MAX) >> shift))
        }
        _ => None,
    }
}

/// `const S = new Int32Array(<literal length>)`-style bindings whose element
/// reads are provably integers: the binding is a `const` (never reassigned),
/// the element kind fits a signed i32, the length is a compile-time literal,
/// and the binding is only ever used as an element-access receiver (`S[...]`
/// reads and writes) — so nothing can alias it, detach its buffer, or swap
/// the value behind it. Returns `id → length`.
fn collect_const_int_ta_views(stmts: &[perry_hir::Stmt]) -> HashMap<u32, i64> {
    use perry_hir::{Expr, Stmt};
    let mut views: HashMap<u32, i64> = HashMap::new();
    fn seed_stmt(stmt: &Stmt, views: &mut HashMap<u32, i64>) {
        if let Stmt::Let {
            id,
            mutable: false,
            init:
                Some(Expr::TypedArrayNew {
                    kind,
                    arg: Some(arg),
                }),
            ..
        } = stmt
        {
            let len = match arg.as_ref() {
                Expr::Integer(n) => Some(*n),
                Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => Some(*n as i64),
                _ => None,
            };
            if let Some(len) = len {
                if typed_array_kind_elem_fits_i32(*kind) && (0..=16_000_000).contains(&len) {
                    views.insert(*id, len);
                }
            }
        }
        for_each_child_stmt(stmt, &mut |child| seed_stmt(child, views));
    }
    for stmt in stmts {
        seed_stmt(stmt, &mut views);
    }
    if views.is_empty() {
        return views;
    }
    for stmt in stmts {
        scan_ta_view_escapes_stmt(stmt, &mut views);
    }
    views
}

/// Invoke `f` on every statement nested directly inside `stmt` (branch
/// bodies, loop bodies, catch/finally, switch cases, labels).
fn for_each_child_stmt(stmt: &perry_hir::Stmt, f: &mut dyn FnMut(&perry_hir::Stmt)) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().for_each(&mut *f);
            if let Some(eb) = else_branch {
                eb.iter().for_each(&mut *f);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => body.iter().for_each(&mut *f),
        Stmt::For { init, body, .. } => {
            if let Some(init) = init {
                f(init);
            }
            body.iter().for_each(&mut *f);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().for_each(&mut *f);
            if let Some(c) = catch {
                c.body.iter().for_each(&mut *f);
            }
            if let Some(fin) = finally {
                fin.iter().for_each(&mut *f);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                case.body.iter().for_each(&mut *f);
            }
        }
        Stmt::Labeled { body, .. } => f(body.as_ref()),
        _ => {}
    }
}

/// Remove from `views` every binding used anywhere other than as the direct
/// receiver of an element access. A bare `LocalGet` in any other position
/// (call argument, property access, capture, assignment source, …) can alias
/// or detach the array, so the window proof no longer holds.
fn scan_ta_view_escapes_stmt(stmt: &perry_hir::Stmt, views: &mut HashMap<u32, i64>) {
    use perry_hir::Stmt;
    if let Stmt::Let { init: Some(e), .. } = stmt {
        scan_ta_view_escapes_expr(e, views);
    }
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => {
            scan_ta_view_escapes_expr(e, views);
        }
        Stmt::If { condition, .. } => scan_ta_view_escapes_expr(condition, views),
        Stmt::While { condition, .. } | Stmt::DoWhile { condition, .. } => {
            scan_ta_view_escapes_expr(condition, views);
        }
        Stmt::For {
            condition, update, ..
        } => {
            if let Some(c) = condition {
                scan_ta_view_escapes_expr(c, views);
            }
            if let Some(u) = update {
                scan_ta_view_escapes_expr(u, views);
            }
        }
        Stmt::Switch { discriminant, .. } => scan_ta_view_escapes_expr(discriminant, views),
        _ => {}
    }
    for_each_child_stmt(stmt, &mut |child| scan_ta_view_escapes_stmt(child, views));
}

fn scan_ta_view_escapes_expr(e: &perry_hir::Expr, views: &mut HashMap<u32, i64>) {
    use perry_hir::Expr;
    match e {
        // Receiver position of an element access is the one allowed use.
        Expr::IndexGet { object, index } => {
            if !matches!(object.as_ref(), Expr::LocalGet(_)) {
                scan_ta_view_escapes_expr(object, views);
            }
            scan_ta_view_escapes_expr(index, views);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            if !matches!(object.as_ref(), Expr::LocalGet(_)) {
                scan_ta_view_escapes_expr(object, views);
            }
            scan_ta_view_escapes_expr(index, views);
            scan_ta_view_escapes_expr(value, views);
        }
        // `S[k] = v` lowers as PutValueSet with `target` and `receiver` both
        // the receiver local — receiver-position uses, like IndexSet's object.
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            if !matches!(target.as_ref(), Expr::LocalGet(_)) {
                scan_ta_view_escapes_expr(target, views);
            }
            if !matches!(receiver.as_ref(), Expr::LocalGet(_)) {
                scan_ta_view_escapes_expr(receiver, views);
            }
            scan_ta_view_escapes_expr(key, views);
            scan_ta_view_escapes_expr(value, views);
        }
        Expr::LocalGet(id) => {
            views.remove(id);
        }
        Expr::LocalSet(id, value) => {
            views.remove(id);
            scan_ta_view_escapes_expr(value, views);
        }
        Expr::Closure { body, .. } => {
            for stmt in body {
                scan_ta_view_escapes_stmt(stmt, views);
            }
            perry_hir::walker::walk_expr_children(e, &mut |child| {
                scan_ta_view_escapes_expr(child, views);
            });
        }
        _ => {
            perry_hir::walker::walk_expr_children(e, &mut |child| {
                scan_ta_view_escapes_expr(child, views);
            });
        }
    }
}

/// `S[idx]` where `S` is a tracked const int-typed-array view and `idx`'s
/// static window is inside `[0, length)` — an integer by construction.
fn is_proven_int_ta_load(views: &HashMap<u32, i64>, e: &perry_hir::Expr) -> bool {
    use perry_hir::Expr;
    let Expr::IndexGet { object, index } = e else {
        return false;
    };
    let Expr::LocalGet(id) = object.as_ref() else {
        return false;
    };
    let Some(&len) = views.get(id) else {
        return false;
    };
    static_index_window(index).is_some_and(|(lo, hi)| lo >= 0 && hi < len)
}

/// Walk all Lets and seed ids whose init is a proven in-window int-typed-array
/// element load (`const a = S[x & 0xff]` — the bcryptjs Blowfish shape).
fn collect_int_ta_load_let_ids(
    stmts: &[perry_hir::Stmt],
    views: &HashMap<u32, i64>,
    out: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    for stmt in stmts {
        if let Stmt::Let {
            id,
            init: Some(init),
            ..
        } = stmt
        {
            if is_proven_int_ta_load(views, init) {
                out.insert(*id);
            }
        }
        for_each_child_stmt(stmt, &mut |child| {
            collect_int_ta_load_let_ids(std::slice::from_ref(child), views, out);
        });
    }
}

pub fn collect_integer_locals(
    stmts: &[perry_hir::Stmt],
    flat_const_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
    arg_dependent_clamp_fn_ids: &HashSet<u32>,
) -> HashSet<u32> {
    let mut candidates: HashSet<u32> = HashSet::new();

    // Issue #50 bridge: pre-compute which locals are row-aliases of
    // flat-const 2D int arrays BEFORE collecting integer let ids, since
    // `collect_integer_let_ids` needs to recognize `let k = krow[j]`
    // (where krow is a flat-const row alias) as an int-producing init.
    let mut flat_row_alias_ids: HashSet<u32> = HashSet::new();
    collect_flat_row_aliases(stmts, flat_const_ids, &mut flat_row_alias_ids);

    collect_integer_let_ids(
        stmts,
        &mut candidates,
        flat_const_ids,
        &flat_row_alias_ids,
        clamp_fn_ids,
    );

    // `const a = S[x & 0xff]` where `S` is a const int-typed-array view and
    // the index window is statically inside the array: the load is an integer
    // by construction, so seed it like any other int-producing Let. The
    // provenance judge exempts these inits below for the same reason.
    let int_ta_views = collect_const_int_ta_views(stmts);
    collect_int_ta_load_let_ids(stmts, &int_ta_views, &mut candidates);

    // Forward closure pass: extend the seed set with Lets whose init is
    // `is_int32_producing_expr` against the current candidate set.
    // The initial `collect_integer_let_ids` only seeds on syntactic
    // patterns (Integer literals, `(expr) | 0`, clamp calls, …) but
    // misses transitive int-stable Lets like `const hi = W - 1` where
    // `W` is itself a candidate. Iterate to a fixed point so chains
    // such as `const W = 3840` → `const hi = W - 1` → uses-of-hi
    // propagate cleanly.
    //
    // image_convolution's clampIdx-inlined `xx`/`yy` rely on this:
    // their write-set includes `LocalSet(xx, LocalGet(hi))`, and
    // without `hi` in the int-stable set the disqualifier marks the
    // assignment as non-int-producing and removes `xx`/`yy` from the
    // set — taking down the i32 shadow on every downstream use of
    // `idx = (row + xx) * 3` and forcing the inner kernel's address
    // generation back into double.
    loop {
        let before = candidates.len();
        collect_extra_integer_let_ids(
            stmts,
            &mut candidates,
            flat_const_ids,
            &flat_row_alias_ids,
            clamp_fn_ids,
        );
        if candidates.len() == before {
            break;
        }
    }

    // Provenance-based disqualification (see module comment). One walk
    // judges every candidate's defining expressions against the optimistic
    // set, recording which other candidates each verdict relied on; a
    // worklist then propagates removals through those reverse-dependency
    // edges to a fixed point. The judgment is monotone in the candidate set
    // (it only relies *positively* on membership), so judging once against
    // the optimistic set and pruning transitively is exact — no repeated
    // full rescans of the function body per disqualification.
    let mut localset_written: HashSet<u32> = HashSet::new();
    collect_localset_ids_in_stmts(stmts, &mut localset_written);

    let mut judge = ProvenanceJudge {
        candidates: &candidates,
        localset_written: &localset_written,
        flat_const_ids,
        flat_row_alias_ids: &flat_row_alias_ids,
        clamp_fn_ids,
        arg_dependent_clamp_fn_ids,
        int_ta_views: &int_ta_views,
        dependents: HashMap::new(),
        disqualified: HashSet::new(),
        closure_written: HashSet::new(),
    };
    judge.walk_stmts(stmts);

    let ProvenanceJudge {
        dependents,
        mut disqualified,
        closure_written,
        ..
    } = judge;
    // Locals written inside a closure body lose integer-ness in the
    // enclosing scope unconditionally (the closure body gets its own
    // analysis pass; this matches the historical unfiltered closure-body
    // collection in `collect_localset_ids_in_expr_filtered`).
    for id in closure_written {
        if candidates.contains(&id) {
            disqualified.insert(id);
        }
    }
    let mut worklist: Vec<u32> = disqualified.iter().copied().collect();
    while let Some(gone) = worklist.pop() {
        if let Some(dependent_ids) = dependents.get(&gone) {
            for &dep in dependent_ids {
                if disqualified.insert(dep) {
                    worklist.push(dep);
                }
            }
        }
    }
    candidates.retain(|id| !disqualified.contains(id));
    candidates
}

/// Single-pass obligation collector + judge for the disqualification phase.
/// A candidate's "obligations" are its `Let` init (unless it's the optimistic
/// `mutable`-`undefined`-with-writes scaffolding seed, whose real values are
/// the writes) and every `LocalSet` rhs targeting it. Each obligation is
/// judged with `int32_producing_deps`; a failure lands the id in
/// `disqualified`, a success records reverse-dependency edges in `dependents`.
struct ProvenanceJudge<'a> {
    candidates: &'a HashSet<u32>,
    localset_written: &'a HashSet<u32>,
    flat_const_ids: &'a HashSet<u32>,
    flat_row_alias_ids: &'a HashSet<u32>,
    clamp_fn_ids: &'a HashSet<u32>,
    arg_dependent_clamp_fn_ids: &'a HashSet<u32>,
    /// Const int-typed-array views (`id → length`) whose in-window element
    /// loads are integers by construction — obligations whose rhs is such a
    /// load pass without deps.
    int_ta_views: &'a HashMap<u32, i64>,
    /// dep local id → candidate ids whose integer verdict relied on it.
    dependents: HashMap<u32, Vec<u32>>,
    /// Candidates with at least one failed obligation.
    disqualified: HashSet<u32>,
    /// Ids `LocalSet` anywhere inside a closure body in these stmts.
    closure_written: HashSet<u32>,
}

impl ProvenanceJudge<'_> {
    fn judge_obligation(&mut self, id: u32, rhs: &Expr) {
        // A proven in-window int-typed-array load is an integer regardless of
        // any candidate's status — pass with no deps (the view binding is a
        // never-escaping const, so nothing can invalidate it).
        if is_proven_int_ta_load(self.int_ta_views, rhs) {
            return;
        }
        let mut deps: HashSet<u32> = HashSet::new();
        if int32_producing_deps(
            rhs,
            self.candidates,
            self.flat_const_ids,
            self.flat_row_alias_ids,
            self.clamp_fn_ids,
            self.arg_dependent_clamp_fn_ids,
            &mut deps,
        ) {
            for dep in deps {
                self.dependents.entry(dep).or_default().push(id);
            }
        } else {
            self.disqualified.insert(id);
        }
    }

    fn walk_stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            match s {
                Stmt::Let {
                    id,
                    init: Some(init),
                    mutable,
                    ..
                } => {
                    // The `let x = undefined; …writes…` scaffolding seed is
                    // admitted optimistically — its init is exempt and its
                    // writes are the obligations. A *write-free* undefined
                    // init has no writes to vouch for it and must fail.
                    let optimistic_undefined_seed = *mutable
                        && matches!(init, Expr::Undefined)
                        && self.localset_written.contains(id);
                    if self.candidates.contains(id) && !optimistic_undefined_seed {
                        self.judge_obligation(*id, init);
                    }
                    self.walk_expr(init);
                }
                Stmt::Let { init: None, .. } => {}
                Stmt::Expr(e) | Stmt::Throw(e) => self.walk_expr(e),
                Stmt::Return(opt) => {
                    if let Some(e) = opt {
                        self.walk_expr(e);
                    }
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    self.walk_expr(condition);
                    self.walk_stmts(then_branch);
                    if let Some(eb) = else_branch {
                        self.walk_stmts(eb);
                    }
                }
                Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                    self.walk_expr(condition);
                    self.walk_stmts(body);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(init_stmt) = init {
                        self.walk_stmts(std::slice::from_ref(init_stmt));
                    }
                    if let Some(cond) = condition {
                        self.walk_expr(cond);
                    }
                    if let Some(upd) = update {
                        self.walk_expr(upd);
                    }
                    self.walk_stmts(body);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    self.walk_stmts(body);
                    if let Some(c) = catch {
                        self.walk_stmts(&c.body);
                    }
                    if let Some(f) = finally {
                        self.walk_stmts(f);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    self.walk_expr(discriminant);
                    for c in cases {
                        if let Some(t) = &c.test {
                            self.walk_expr(t);
                        }
                        self.walk_stmts(&c.body);
                    }
                }
                Stmt::Labeled { body, .. } => {
                    self.walk_stmts(std::slice::from_ref(body.as_ref()));
                }
                _ => {}
            }
        }
    }

    fn walk_expr(&mut self, e: &Expr) {
        match e {
            Expr::LocalSet(id, value) => {
                if self.candidates.contains(id) {
                    self.judge_obligation(*id, value);
                }
                self.walk_expr(value);
            }
            Expr::Closure { body, .. } => {
                collect_localset_ids_in_stmts(body, &mut self.closure_written);
                // The centralized walker visits param-default exprs (but
                // not the body — handled above).
                perry_hir::walker::walk_expr_children(e, &mut |child| self.walk_expr(child));
            }
            _ => {
                perry_hir::walker::walk_expr_children(e, &mut |child| self.walk_expr(child));
            }
        }
    }
}

/// Disqualification-side judgment: returns `true` if `e` is int-producing
/// given the current candidate set, accumulating into `deps` every local id
/// whose candidate-ness the verdict relied on. `collect_integer_locals`
/// prunes the judged local transitively when any of those deps is later
/// disqualified.
///
/// Mirrors `is_int32_producing_expr` (the admission-side judgment), except
/// it is strict where admission is optimistic:
///   - `Expr::Update`: int-producing only when the updated local is itself
///     a candidate (admission says unconditionally yes; `++` on a
///     non-integer local yields whatever `ToNumber` gives — possibly a
///     fractional or NaN value).
///   - Calls to *argument-dependent* clamp functions (`clamp3`-shaped: they
///     return one of their arguments verbatim): every argument must be
///     int-producing, and the argument deps are recorded. `clampU8`-shaped
///     and `returns_integer` functions coerce internally (`| 0` / bitwise on
///     every value-returning path) and stay argument-independent.
#[allow(clippy::too_many_arguments)]
fn int32_producing_deps(
    e: &perry_hir::Expr,
    candidates: &HashSet<u32>,
    flat_const_ids: &HashSet<u32>,
    flat_row_alias_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
    arg_dependent_clamp_fn_ids: &HashSet<u32>,
    deps: &mut HashSet<u32>,
) -> bool {
    let recurse = |sub: &Expr, deps: &mut HashSet<u32>| {
        int32_producing_deps(
            sub,
            candidates,
            flat_const_ids,
            flat_row_alias_ids,
            clamp_fn_ids,
            arg_dependent_clamp_fn_ids,
            deps,
        )
    };
    match e {
        // #6319: only a literal that actually fits in i32 proves an int32
        // value. `let x = 3000000000` is integral but not int32, and admitting
        // it here let every copy of `x` inherit an unearned i32 shadow.
        Expr::Integer(n) => super::i32_locals::integer_literal_fits_i32(*n),
        Expr::Update { id, .. } if candidates.contains(id) => {
            deps.insert(*id);
            true
        }
        Expr::Binary { op, right, .. }
            if matches!(op, BinaryOp::BitOr | BinaryOp::UShr)
                && matches!(right.as_ref(), Expr::Integer(0)) =>
        {
            true
        }
        Expr::Binary { op, left, right }
            if matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul) =>
        {
            recurse(left, deps) && recurse(right, deps)
        }
        Expr::Call { callee, args, .. } => {
            if let Expr::FuncRef(fid) = callee.as_ref() {
                if !clamp_fn_ids.contains(fid) {
                    return false;
                }
                if arg_dependent_clamp_fn_ids.contains(fid) {
                    args.iter().all(|a| recurse(a, deps))
                } else {
                    true
                }
            } else {
                false
            }
        }
        Expr::Binary { op, .. } => matches!(
            op,
            BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
                | BinaryOp::UShr
        ),
        Expr::LocalGet(id) if candidates.contains(id) => {
            deps.insert(*id);
            true
        }
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => true,
        Expr::MathImul(_, _) => true,
        // Issue #50 bridge: element access on a flat-const 2D int array
        // produces i32. The flat-const facts are immutable within this
        // analysis (never-mutated module consts), so no local deps.
        Expr::IndexGet { object, .. } => match object.as_ref() {
            Expr::IndexGet { object: inner, .. } => {
                matches!(inner.as_ref(), Expr::LocalGet(id) if flat_const_ids.contains(id))
            }
            Expr::LocalGet(id) => flat_row_alias_ids.contains(id),
            _ => false,
        },
        _ => false,
    }
}

/// Walk all `Stmt::Let { id, init: Some(e), .. }` and add `id` to
/// `out` when `e` is `is_int32_producing_expr` against the *current*
/// `out` set. Used by `collect_integer_locals` to take the
/// syntactic seed set's transitive closure: e.g. `const W = 3840` is
/// seeded on the initial pass, then `const hi = W - 1` lands here on
/// the second pass because `W` is already in the set, and any Let
/// whose init reduces to `is_int32_producing_expr` over `hi` lands
/// on the third pass.
pub fn collect_extra_integer_let_ids(
    stmts: &[perry_hir::Stmt],
    out: &mut HashSet<u32>,
    flat_const_ids: &HashSet<u32>,
    flat_row_alias_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                init: Some(init),
                ..
            }
                // Same `>>> 0` exclusion as the syntactic seed in
                // `collect_integer_let_ids`: u32 values can't round-trip
                // through an i32 slot.
                if !is_ushr_zero(init)
                    && !out.contains(id)
                    && is_int32_producing_expr(
                        init,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    )
                => {
                    out.insert(*id);
                }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_extra_integer_let_ids(
                    then_branch,
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
                if let Some(eb) = else_branch {
                    collect_extra_integer_let_ids(
                        eb,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_extra_integer_let_ids(
                        std::slice::from_ref(init_stmt),
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
                collect_extra_integer_let_ids(
                    body,
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_extra_integer_let_ids(
                    body,
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_extra_integer_let_ids(
                    body,
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
                if let Some(c) = catch {
                    collect_extra_integer_let_ids(
                        &c.body,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
                if let Some(f) = finally {
                    collect_extra_integer_let_ids(
                        f,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    collect_extra_integer_let_ids(
                        &c.body,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_extra_integer_let_ids(
                    std::slice::from_ref(body.as_ref()),
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
            }
            _ => {}
        }
    }
}

pub fn collect_flat_row_aliases(
    stmts: &[perry_hir::Stmt],
    flat_const_ids: &HashSet<u32>,
    out: &mut HashSet<u32>,
) {
    use perry_hir::{Expr, Stmt};
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                init: Some(Expr::IndexGet { object, .. }),
                mutable: false,
                ..
            } => {
                if let Expr::LocalGet(const_id) = object.as_ref() {
                    if flat_const_ids.contains(const_id) {
                        out.insert(*id);
                    }
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_flat_row_aliases(then_branch, flat_const_ids, out);
                if let Some(eb) = else_branch {
                    collect_flat_row_aliases(eb, flat_const_ids, out);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_flat_row_aliases(std::slice::from_ref(init_stmt), flat_const_ids, out);
                }
                collect_flat_row_aliases(body, flat_const_ids, out);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_flat_row_aliases(body, flat_const_ids, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_flat_row_aliases(body, flat_const_ids, out);
                if let Some(catch) = catch {
                    collect_flat_row_aliases(&catch.body, flat_const_ids, out);
                }
                if let Some(finally) = finally {
                    collect_flat_row_aliases(finally, flat_const_ids, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_flat_row_aliases(&case.body, flat_const_ids, out);
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_flat_row_aliases(std::slice::from_ref(body.as_ref()), flat_const_ids, out);
            }
            _ => {}
        }
    }
}

/// Returns `true` if evaluating `e` yields a value that will already be
/// integer-valued — so writing it into a local's i32 slot is lossless.
///
/// This is the *admission-side* judgment: it may be optimistic (e.g.
/// `Expr::Update` and clamp calls are accepted unconditionally) because every
/// admitted candidate is re-judged with the strict, provenance-recording
/// `int32_producing_deps` during disqualification — see the module comment.
///
/// Accepted shapes:
///   - `Expr::Integer(n)` **when `n` fits in i32** (#6319). `Expr::Integer`
///     carries an `i64`, so `let x = 3000000000` and `let x =
///     Number.MAX_SAFE_INTEGER` parse into it too — integral, but not int32.
///     Admitting them let a plain copy (`let y = 0; y = x`) be judged
///     i32-bounded and take a wrapping i32 shadow.
///   - `(expr) | 0` and `(expr) >>> 0`: the JS ToInt32 / ToUint32 idiom —
///     always yields a 32-bit integer regardless of the inner expression.
///   - Pure bitwise ops (`&`, `|`, `^`, `<<`, `>>`, `>>>`): per JS spec
///     these coerce both operands to int32 and return int32.
///   - `Expr::Update`: `++` / `--` on an integer-stable local (admission
///     doesn't verify the target; the disqualification judgment does).
///   - (issue #49) `LocalGet(id)` when `id` is itself in `known_int_locals` —
///     enables the accumulator pattern `acc = acc + int_expr` without
///     requiring a `| 0` wrapper on every write.
///   - (issue #49) `Uint8ArrayGet` / `BufferIndexGet`: typed-array byte
///     reads return u8 values; always fit in i32.
///   - (issue #49) `Add` / `Sub` / `Mul` when both operands are
///     int-producing. The sum/product may overflow i32, but the existing
///     i32-slot machinery already accepts this risk — the double slot is
///     maintained in parallel and reads past i32::MAX were already wrong
///     for `| 0`-written accumulators.
///
/// Rejected: everything else (notably `Div`/`Mod` without a `|0` wrapper,
/// bare floats, calls returning doubles, etc.) because they can produce
/// non-integer doubles at runtime.
pub fn is_int32_producing_expr(
    e: &perry_hir::Expr,
    known_int_locals: &HashSet<u32>,
    flat_const_ids: &HashSet<u32>,
    flat_row_alias_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
) -> bool {
    use perry_hir::{BinaryOp, Expr};
    match e {
        // #6319: an `Expr::Integer` holds an i64. Only accept it when it really
        // is an int32 — otherwise the forward-closure pass re-admits the very
        // `let x = 3000000000` that the syntactic seed now rejects.
        Expr::Integer(n) => super::i32_locals::integer_literal_fits_i32(*n),
        Expr::Update { .. } => true,
        Expr::Binary { op, right, .. }
            if matches!(op, BinaryOp::BitOr | BinaryOp::UShr)
                && matches!(right.as_ref(), Expr::Integer(0)) =>
        {
            true
        }
        Expr::Binary { op, left, right }
            if matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul) =>
        {
            is_int32_producing_expr(
                left,
                known_int_locals,
                flat_const_ids,
                flat_row_alias_ids,
                clamp_fn_ids,
            ) && is_int32_producing_expr(
                right,
                known_int_locals,
                flat_const_ids,
                flat_row_alias_ids,
                clamp_fn_ids,
            )
        }
        Expr::Call { callee, .. } => {
            if let Expr::FuncRef(fid) = callee.as_ref() {
                clamp_fn_ids.contains(fid)
            } else {
                false
            }
        }
        Expr::Binary { op, .. } => matches!(
            op,
            BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
                | BinaryOp::UShr
        ),
        Expr::LocalGet(id) => known_int_locals.contains(id),
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => true,
        Expr::MathImul(_, _) => true, // Math.imul always returns i32
        // Issue #50 bridge: element access on a flat-const 2D int array
        // produces i32. Two shapes:
        //   - inline `X[i][j]`: IndexGet(IndexGet(LocalGet(X), i), j)
        //   - aliased `krow[j]`: IndexGet(LocalGet(alias), j)
        Expr::IndexGet { object, .. } => match object.as_ref() {
            Expr::IndexGet { object: inner, .. } => {
                matches!(inner.as_ref(), Expr::LocalGet(id) if flat_const_ids.contains(id))
            }
            Expr::LocalGet(id) => flat_row_alias_ids.contains(id),
            _ => false,
        },
        _ => false,
    }
}
