//! Flow analysis: locals that are integer-valued **within i64 range**, even
//! though they are *not* provably within i32 range.
//!
//! ## Why this is a separate set from `integer_locals`
//!
//! `collectors::integer_locals` answers "is this local i32-RANGE?", because its
//! consumers (`needs_i32_slot`, `canonical_i32_value_eligible` in
//! `stmt/let_stmt.rs`, the loop-counter lanes in `stmt/loops.rs`) put the value
//! into an **i32 shadow slot**. A local like `a` in `a = a + 1` is correctly
//! *rejected* there: an unbounded increment chain can exceed i32, and admitting
//! it would silently truncate. That judgment must not be widened.
//!
//! But the `%` integer fast path in `expr/binary.rs` converts with
//! `fptosi double -> **i64**`, so it does not need i32-range at all — it needs
//! "integer-valued, and small enough that `fptosi` to i64 is exact and
//! in-range". It was asking `integer_locals` the wrong question, so
//! `bench_bitwise`'s `a % 1000` / `(a * 3) % 10000` fell through to
//! `frem double`, which on AArch64 is not an instruction and lowers to a
//! `bl _fmod` libm call — the dominant cost in that benchmark.
//!
//! This module answers the *right* question for that consumer, and nothing
//! else consumes it.
//!
//! ## Admission rule
//!
//! A local `L` is admitted only when ALL of the following hold:
//!
//! 1. `L` is declared by exactly one `Stmt::Let` whose init is an
//!    `Expr::Integer(v)` literal with `|v| <= 2^31`. (Params are excluded by
//!    construction — their incoming argument is an unmodeled write. A second
//!    `Let` for the same id, or a non-literal init, rejects.)
//! 2. Every write to `L` anywhere in the function is one of:
//!      - `LocalSet(L, Integer(v))`                       with `|v| <= 2^31`
//!      - `LocalSet(L, Add(LocalGet(L), Integer(d)))`     with `|d| <= 64`
//!      - `LocalSet(L, Add(Integer(d), LocalGet(L)))`     with `|d| <= 64`
//!      - `LocalSet(L, Sub(LocalGet(L), Integer(d)))`     with `|d| <= 64`
//!      - `Update { id: L, .. }`                          (`++` / `--`, `d = 1`)
//!    Any other write shape rejects `L`. In particular
//!    `Sub(Integer(d), LocalGet(L))` (`L = d - L`) is **rejected**: it negates
//!    `L`, so a step is no longer a bounded *translation* and the saturation
//!    argument below collapses. `Mul` is rejected outright — repeated
//!    multiplication leaves i64 range in a few dozen iterations.
//! 3. `L` is never written inside a closure body and never appears in a
//!    closure's `mutable_captures` (mirrors how `integer_locals.rs` treats
//!    `closure_written`), and is not a `catch` clause parameter.
//!
//! ## Soundness: why the value stays inside i64
//!
//! `L` starts as an i64-representable integer, and rule (2) makes every write
//! either a reset to a literal `<= 2^31` or a translation by a compile-time
//! constant `d` with `|d| <= D <= 64`. So `L` is always an integral f64, and it
//! can only grow by `<= D` per step.
//!
//! The bound is not a hand-wave about iteration counts — IEEE-754 makes it a
//! hard ceiling. For an f64 `v` with `2^e <= |v| < 2^(e+1)`,
//! `ulp(v) = 2^(e-52)`. Once `ulp(v) >= 4D` — i.e. once `e >= 54 + log2(D)` —
//! adding `+-d` with `|d| <= D <= ulp/4` is *strictly* inside the
//! round-to-nearest half-ulp window, so `v + d` rounds back to `v` exactly.
//! The value becomes a **fixed point and can never grow again**. Approaching
//! that threshold from below, one final step can overshoot by at most
//! `D + ulp/2`, so
//!
//! ```text
//!     |L| <= max(2^31, 2^(55 + log2 D))   for all time.
//! ```
//!
//! We record `56 + ceil_log2(D)` per local (one bit of slack) as that local's
//! magnitude bound, and `MAX_DELTA = 64` caps it at `62` bits. The `%` gate
//! admits an expression only when its derived magnitude is `<= 2^62`, which
//! keeps `fptosi double -> i64` exact and in range (`i64::MAX ~= 2^63`) with a
//! full bit to spare. A local whose writes are all literal resets never grows
//! at all and is recorded at `31`.
//!
//! This is deliberately an under-approximation: anything unproven is simply
//! left to `frem`, which is always correct.

use std::collections::{HashMap, HashSet};

use perry_hir::{BinaryOp, Expr, Stmt};

/// Largest `|delta|` admitted for a `L = L +- d` write. Caps the recorded
/// magnitude at `56 + 6 = 62` bits, which is the `%` gate's ceiling.
const MAX_DELTA: i64 = 64;

/// Largest `|v|` admitted for a literal initialiser / literal reset.
const MAX_LITERAL: i64 = 1 << 31;

/// Magnitude recorded for a local whose every write is a literal reset — it
/// never grows, so `|L| <= 2^31`.
const LITERAL_ONLY_BITS: u32 = 31;

/// `56 + ceil_log2(D)`: the saturation ceiling of a `+-D` translation chain
/// (`2^(55 + log2 D)`) plus one bit of slack.
const STEP_BITS_BASE: u32 = 56;

/// Smallest `bits` with `|v| <= 2^bits`. `ceil(log2(|v|))`, and `0` for `v == 0`.
pub(crate) fn ceil_log2_abs(v: i64) -> u32 {
    let m = v.unsigned_abs();
    if m <= 1 {
        return 0;
    }
    64 - (m - 1).leading_zeros()
}

/// Per-local accumulated state during the walk.
#[derive(Clone, Copy)]
struct Cand {
    /// Largest `|d|` seen across `L = L +- d` writes; `0` if only literal
    /// resets have been seen.
    max_delta: i64,
}

impl Cand {
    fn magnitude_bits(self) -> u32 {
        if self.max_delta == 0 {
            LITERAL_ONLY_BITS
        } else {
            STEP_BITS_BASE + ceil_log2_abs(self.max_delta)
        }
    }
}

/// Locals that are integer-valued within i64 range, mapped to a conservative
/// upper bound on `log2(|value|)`. Consumed only by the `%` integer fast path
/// via `type_analysis::numeric::integer_magnitude_bits`.
pub fn collect_int_valued_i64_locals(stmts: &[Stmt]) -> HashMap<u32, u32> {
    let mut w = Walk {
        cands: HashMap::new(),
        rejected: HashSet::new(),
    };
    // Pass 1: seed candidates from integer-literal `Let` inits.
    w.seed_stmts(stmts);
    // Pass 2: judge every write in the function against the whitelist.
    w.judge_stmts(stmts);

    w.cands
        .into_iter()
        .filter(|(id, _)| !w.rejected.contains(id))
        .map(|(id, c)| (id, c.magnitude_bits()))
        .collect()
}

struct Walk {
    cands: HashMap<u32, Cand>,
    rejected: HashSet<u32>,
}

impl Walk {
    fn reject(&mut self, id: u32) {
        self.rejected.insert(id);
    }

    // ---- Pass 1: seeding -------------------------------------------------

    fn seed_stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            match s {
                Stmt::Let { id, init, .. } => {
                    let ok = matches!(init, Some(Expr::Integer(v)) if v.unsigned_abs() <= MAX_LITERAL as u64);
                    if ok {
                        // A second `Let` for the same id is not expected
                        // (LocalIds are unique per function); treat it as an
                        // unmodeled rebinding and reject rather than trust it.
                        if self.cands.insert(*id, Cand { max_delta: 0 }).is_some() {
                            self.reject(*id);
                        }
                    } else {
                        self.reject(*id);
                    }
                    if let Some(e) = init {
                        self.seed_expr(e);
                    }
                }
                Stmt::Expr(e) | Stmt::Throw(e) => self.seed_expr(e),
                Stmt::Return(opt) => {
                    if let Some(e) = opt {
                        self.seed_expr(e);
                    }
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    self.seed_expr(condition);
                    self.seed_stmts(then_branch);
                    if let Some(eb) = else_branch {
                        self.seed_stmts(eb);
                    }
                }
                Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                    self.seed_expr(condition);
                    self.seed_stmts(body);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(i) = init {
                        self.seed_stmts(std::slice::from_ref(i));
                    }
                    if let Some(c) = condition {
                        self.seed_expr(c);
                    }
                    if let Some(u) = update {
                        self.seed_expr(u);
                    }
                    self.seed_stmts(body);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    self.seed_stmts(body);
                    if let Some(c) = catch {
                        // A catch parameter is an unmodeled binding.
                        if let Some((pid, _)) = &c.param {
                            self.reject(*pid);
                        }
                        self.seed_stmts(&c.body);
                    }
                    if let Some(f) = finally {
                        self.seed_stmts(f);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    self.seed_expr(discriminant);
                    for c in cases {
                        if let Some(t) = &c.test {
                            self.seed_expr(t);
                        }
                        self.seed_stmts(&c.body);
                    }
                }
                Stmt::Labeled { body, .. } => self.seed_stmts(std::slice::from_ref(body.as_ref())),
                _ => {}
            }
        }
    }

    /// Seeding only needs to reach `Let`s nested inside closure bodies so that
    /// their ids are *known*; the judging pass rejects anything a closure
    /// writes, so no candidate can survive on a closure-local basis.
    fn seed_expr(&mut self, e: &Expr) {
        if let Expr::Closure { body, .. } = e {
            self.seed_stmts(body);
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| self.seed_expr(c));
    }

    // ---- Pass 2: judging every write -------------------------------------

    fn judge_stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            match s {
                Stmt::Let { init, .. } => {
                    if let Some(e) = init {
                        self.judge_expr(e);
                    }
                }
                Stmt::Expr(e) | Stmt::Throw(e) => self.judge_expr(e),
                Stmt::Return(opt) => {
                    if let Some(e) = opt {
                        self.judge_expr(e);
                    }
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    self.judge_expr(condition);
                    self.judge_stmts(then_branch);
                    if let Some(eb) = else_branch {
                        self.judge_stmts(eb);
                    }
                }
                Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                    self.judge_expr(condition);
                    self.judge_stmts(body);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(i) = init {
                        self.judge_stmts(std::slice::from_ref(i));
                    }
                    if let Some(c) = condition {
                        self.judge_expr(c);
                    }
                    if let Some(u) = update {
                        self.judge_expr(u);
                    }
                    self.judge_stmts(body);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    self.judge_stmts(body);
                    if let Some(c) = catch {
                        self.judge_stmts(&c.body);
                    }
                    if let Some(f) = finally {
                        self.judge_stmts(f);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    self.judge_expr(discriminant);
                    for c in cases {
                        if let Some(t) = &c.test {
                            self.judge_expr(t);
                        }
                        self.judge_stmts(&c.body);
                    }
                }
                Stmt::Labeled { body, .. } => self.judge_stmts(std::slice::from_ref(body.as_ref())),
                _ => {}
            }
        }
    }

    fn judge_expr(&mut self, e: &Expr) {
        match e {
            Expr::LocalSet(id, rhs) => {
                if self.cands.contains_key(id) {
                    match write_delta(*id, rhs) {
                        Some(d) => {
                            let c = self.cands.get_mut(id).expect("candidate present");
                            c.max_delta = c.max_delta.max(d);
                        }
                        None => self.reject(*id),
                    }
                }
                self.judge_expr(rhs);
            }
            Expr::Update { id, .. } => {
                // `++` / `--`: a translation by exactly 1.
                if let Some(c) = self.cands.get_mut(id) {
                    c.max_delta = c.max_delta.max(1);
                }
            }
            Expr::Closure {
                body,
                mutable_captures,
                ..
            } => {
                // A closure can write the local out of line; the enclosing
                // analysis cannot see when. Reject unconditionally, matching
                // `integer_locals.rs`'s `closure_written` handling.
                for id in mutable_captures {
                    self.reject(*id);
                }
                let mut written = HashSet::new();
                collect_written_ids(body, &mut written);
                for id in written {
                    self.reject(id);
                }
                perry_hir::walker::walk_expr_children(e, &mut |c| self.judge_expr(c));
            }
            _ => {
                perry_hir::walker::walk_expr_children(e, &mut |c| self.judge_expr(c));
            }
        }
    }
}

/// Classify a `LocalSet(id, rhs)` write. Returns the translation magnitude
/// (`0` for a literal reset), or `None` when the shape is not admissible.
fn write_delta(id: u32, rhs: &Expr) -> Option<i64> {
    match rhs {
        // Literal reset.
        Expr::Integer(v) if v.unsigned_abs() <= MAX_LITERAL as u64 => Some(0),
        Expr::Binary { op, left, right } => {
            let d = match (op, left.as_ref(), right.as_ref()) {
                // L = L + d   /   L = L - d
                (BinaryOp::Add | BinaryOp::Sub, Expr::LocalGet(l), Expr::Integer(d))
                    if *l == id =>
                {
                    *d
                }
                // L = d + L  (commutative, same translation).
                // NOTE: `L = d - L` is deliberately NOT admitted — it negates
                // L, so the step is not a bounded translation.
                (BinaryOp::Add, Expr::Integer(d), Expr::LocalGet(l)) if *l == id => *d,
                _ => return None,
            };
            let d = d.unsigned_abs();
            if d <= MAX_DELTA as u64 {
                Some(d as i64)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Every local id written (`LocalSet` or `Update`) anywhere in `stmts`,
/// including inside nested closures.
fn collect_written_ids(stmts: &[Stmt], out: &mut HashSet<u32>) {
    struct W<'a>(&'a mut HashSet<u32>);
    impl W<'_> {
        fn stmts(&mut self, stmts: &[Stmt]) {
            for s in stmts {
                match s {
                    Stmt::Let { init, .. } => {
                        if let Some(e) = init {
                            self.expr(e);
                        }
                    }
                    Stmt::Expr(e) | Stmt::Throw(e) => self.expr(e),
                    Stmt::Return(opt) => {
                        if let Some(e) = opt {
                            self.expr(e);
                        }
                    }
                    Stmt::If {
                        condition,
                        then_branch,
                        else_branch,
                    } => {
                        self.expr(condition);
                        self.stmts(then_branch);
                        if let Some(eb) = else_branch {
                            self.stmts(eb);
                        }
                    }
                    Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                        self.expr(condition);
                        self.stmts(body);
                    }
                    Stmt::For {
                        init,
                        condition,
                        update,
                        body,
                    } => {
                        if let Some(i) = init {
                            self.stmts(std::slice::from_ref(i));
                        }
                        if let Some(c) = condition {
                            self.expr(c);
                        }
                        if let Some(u) = update {
                            self.expr(u);
                        }
                        self.stmts(body);
                    }
                    Stmt::Try {
                        body,
                        catch,
                        finally,
                    } => {
                        self.stmts(body);
                        if let Some(c) = catch {
                            self.stmts(&c.body);
                        }
                        if let Some(f) = finally {
                            self.stmts(f);
                        }
                    }
                    Stmt::Switch {
                        discriminant,
                        cases,
                    } => {
                        self.expr(discriminant);
                        for c in cases {
                            if let Some(t) = &c.test {
                                self.expr(t);
                            }
                            self.stmts(&c.body);
                        }
                    }
                    Stmt::Labeled { body, .. } => self.stmts(std::slice::from_ref(body.as_ref())),
                    _ => {}
                }
            }
        }
        fn expr(&mut self, e: &Expr) {
            match e {
                Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                    self.0.insert(*id);
                }
                Expr::Closure { body, .. } => self.stmts(body),
                _ => {}
            }
            perry_hir::walker::walk_expr_children(e, &mut |c| self.expr(c));
        }
    }
    W(out).stmts(stmts);
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;

    fn let_int(id: u32, v: i64) -> Stmt {
        Stmt::Let {
            id,
            name: format!("v{id}"),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Integer(v)),
        }
    }

    fn set(id: u32, rhs: Expr) -> Stmt {
        Stmt::Expr(Expr::LocalSet(id, Box::new(rhs)))
    }

    fn add(id: u32, d: i64) -> Expr {
        Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(id)),
            right: Box::new(Expr::Integer(d)),
        }
    }

    #[test]
    fn ceil_log2_abs_is_an_upper_bound() {
        for v in [0i64, 1, 2, 3, 4, 5, 7, 8, 1000, 1024, 1025, 12345678] {
            let b = ceil_log2_abs(v);
            assert!(
                (v.unsigned_abs() as u128) <= 1u128 << b,
                "{v} > 2^{b} — bound is not an upper bound"
            );
            assert_eq!(b, ceil_log2_abs(-v), "sign must not matter for {v}");
        }
        assert_eq!(ceil_log2_abs(3), 2);
        assert_eq!(ceil_log2_abs(1000), 10);
    }

    #[test]
    fn admits_literal_init_with_unit_steps() {
        // let a = 12345678; a = a + 1; a = 12345678;   (the bench_bitwise shape)
        let stmts = vec![
            let_int(9, 12345678),
            set(9, add(9, 1)),
            set(9, Expr::Integer(12345678)),
        ];
        let out = collect_int_valued_i64_locals(&stmts);
        assert_eq!(out.get(&9), Some(&56), "unit-step local should be 56 bits");
    }

    #[test]
    fn literal_only_writes_stay_at_31_bits() {
        let stmts = vec![let_int(1, 5), set(1, Expr::Integer(7))];
        assert_eq!(collect_int_valued_i64_locals(&stmts).get(&1), Some(&31));
    }

    #[test]
    fn step_widens_the_recorded_magnitude() {
        let stmts = vec![let_int(1, 0), set(1, add(1, 64))];
        assert_eq!(
            collect_int_valued_i64_locals(&stmts).get(&1),
            Some(&62),
            "delta 64 must record 56+6 bits"
        );
    }

    #[test]
    fn rejects_oversized_delta() {
        let stmts = vec![let_int(1, 0), set(1, add(1, 65))];
        assert!(collect_int_valued_i64_locals(&stmts).get(&1).is_none());
    }

    #[test]
    fn rejects_multiplication_write() {
        let stmts = vec![
            let_int(1, 2),
            set(
                1,
                Expr::Binary {
                    op: BinaryOp::Mul,
                    left: Box::new(Expr::LocalGet(1)),
                    right: Box::new(Expr::Integer(3)),
                },
            ),
        ];
        assert!(
            collect_int_valued_i64_locals(&stmts).get(&1).is_none(),
            "L = L * 3 leaves i64 in a few dozen iterations"
        );
    }

    #[test]
    fn rejects_negating_write() {
        // `L = 5 - L` is a reflection, not a translation.
        let stmts = vec![
            let_int(1, 2),
            set(
                1,
                Expr::Binary {
                    op: BinaryOp::Sub,
                    left: Box::new(Expr::Integer(5)),
                    right: Box::new(Expr::LocalGet(1)),
                },
            ),
        ];
        assert!(collect_int_valued_i64_locals(&stmts).get(&1).is_none());
    }

    #[test]
    fn rejects_write_of_another_local() {
        let stmts = vec![let_int(1, 2), let_int(2, 3), set(1, Expr::LocalGet(2))];
        assert!(collect_int_valued_i64_locals(&stmts).get(&1).is_none());
    }

    #[test]
    fn rejects_non_literal_init() {
        let stmts = vec![
            Stmt::Let {
                id: 1,
                name: "a".into(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(1.5)),
            },
            set(1, add(1, 1)),
        ];
        assert!(collect_int_valued_i64_locals(&stmts).get(&1).is_none());
    }

    #[test]
    fn rejects_oversized_literal_init() {
        let stmts = vec![let_int(1, (1i64 << 31) + 1), set(1, add(1, 1))];
        assert!(collect_int_valued_i64_locals(&stmts).get(&1).is_none());
    }

    #[test]
    fn rejects_closure_written_local() {
        let stmts = vec![
            let_int(1, 0),
            Stmt::Expr(Expr::Closure {
                func_id: 0,
                params: vec![],
                return_type: Type::Void,
                body: vec![set(1, add(1, 1))],
                captures: vec![1],
                mutable_captures: vec![],
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: false,
            }),
        ];
        assert!(
            collect_int_valued_i64_locals(&stmts).get(&1).is_none(),
            "a closure can write the local out of line"
        );
    }

    #[test]
    fn rejects_mutable_capture() {
        let stmts = vec![
            let_int(1, 0),
            Stmt::Expr(Expr::Closure {
                func_id: 0,
                params: vec![],
                return_type: Type::Void,
                body: vec![],
                captures: vec![1],
                mutable_captures: vec![1],
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: false,
            }),
        ];
        assert!(collect_int_valued_i64_locals(&stmts).get(&1).is_none());
    }

    #[test]
    fn admits_update_expression_writes() {
        let stmts = vec![
            let_int(1, 0),
            Stmt::Expr(Expr::Update {
                id: 1,
                op: perry_hir::UpdateOp::Increment,
                prefix: false,
            }),
        ];
        assert_eq!(collect_int_valued_i64_locals(&stmts).get(&1), Some(&56));
    }

    #[test]
    fn finds_writes_nested_in_control_flow() {
        // A write buried in an `if` inside a `for` must still be judged.
        let stmts = vec![
            let_int(1, 0),
            Stmt::For {
                init: None,
                condition: None,
                update: None,
                body: vec![Stmt::If {
                    condition: Expr::Bool(true),
                    then_branch: vec![set(
                        1,
                        Expr::Binary {
                            op: BinaryOp::Mul,
                            left: Box::new(Expr::LocalGet(1)),
                            right: Box::new(Expr::Integer(3)),
                        },
                    )],
                    else_branch: None,
                }],
            },
        ];
        assert!(
            collect_int_valued_i64_locals(&stmts).get(&1).is_none(),
            "a nested Mul write must reject, not be skipped"
        );
    }
}
