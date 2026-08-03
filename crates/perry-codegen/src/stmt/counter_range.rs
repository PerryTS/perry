//! Monotone-induction range proof for `for`-loop counters (#7286).
//!
//! A `for` counter whose only writer is the loop's own update slot and whose
//! step never decreases it is bounded on both sides for the whole body:
//!
//!   * **above** by the loop guard, which is re-evaluated at every body entry
//!     (`j < LIMIT` ⟹ `j <= LIMIT - 1` inside the body), and
//!   * **below** by its initial value, because the step is monotone
//!     non-decreasing.
//!
//! The resulting [`IntRangeFact`] is what
//! `expr::index_get::numeric_index_has_integer_array_index_proof` needs to let
//! `sieve[j]` lower to the inline guarded array diamond instead of the fully
//! opaque `js_array_get_index_or_string` /
//! `js_typed_feedback_array_set_index_or_string` runtime-key helpers. Before
//! #7286 only the `i++`-with-integer-literal-start shape produced a fact, so
//! `for (let j = i * i; j < LIMIT; j = j + i)` — the inner loop of
//! `11_prime_sieve`, ~2.12M stores in its timed region — was demoted.
//!
//! **Non-negativity plus an upper bound is the whole proof obligation.** Being
//! an `i32` is neither necessary nor sufficient: `(i * size + k) | 0` is a
//! genuine `i32` and still fails, because `ToInt32`'s range `[-2^31, 2^31-1]`
//! has `min < 0` and a negative index is a named property, not an element.
//!
//! The ctx-dependent queries are behind [`CounterRangeFacts`] so the admission
//! rules are unit-testable without constructing a `FnCtx` (per CLAUDE.md,
//! `crates/*/tests/*.rs` integration suites do not run per-PR).

use perry_hir::{BinaryOp, CompareOp, Expr, Stmt, UpdateOp};

use crate::expr::{IntRange, IntRangeFact};

use super::loops::{expr_mutates_local, stmts_mutate_local};

/// The `FnCtx`-derived facts the counter-range classifier consults.
pub(crate) trait CounterRangeFacts {
    /// Provable integer range of `expr` at the `for` statement.
    fn int_range(&self, expr: &Expr) -> Option<IntRange>;
    /// True when `id` is known to hold a non-negative *integral* Number at
    /// this program point (`ctx.nonnegative_integer_locals`).
    fn is_nonnegative_integer_local(&self, id: u32) -> bool;
    /// True when `id` is captured by some closure. A captured local can be
    /// written by a closure that was *declared outside* the loop body and
    /// merely *called* inside it, which the syntactic mutation walk cannot
    /// see — so a captured local is never accepted as a loop-invariant stride.
    fn is_closure_captured(&self, id: u32) -> bool;
}

impl CounterRangeFacts for crate::expr::FnCtx<'_> {
    fn int_range(&self, expr: &Expr) -> Option<IntRange> {
        crate::expr::int_range_expr(self, expr)
    }

    fn is_nonnegative_integer_local(&self, id: u32) -> bool {
        self.nonnegative_integer_locals.contains(&id)
    }

    fn is_closure_captured(&self, id: u32) -> bool {
        self.closure_captures.contains_key(&id)
    }
}

/// How the loop's update slot advances the counter.
#[derive(Debug)]
pub(crate) enum CounterStep<'a> {
    /// `i++` — stride exactly `+1`.
    Increment,
    /// `j = j + <stride>` / `j = <stride> + j`. The stride still has to be
    /// proven non-negative and loop-invariant.
    Add(&'a Expr),
}

/// Match the update slot against the two monotone step shapes.
pub(crate) fn counter_step(update: &Expr, counter_id: u32) -> Option<CounterStep<'_>> {
    match update {
        Expr::Update {
            id,
            op: UpdateOp::Increment,
            ..
        } if *id == counter_id => Some(CounterStep::Increment),
        Expr::LocalSet(id, value) if *id == counter_id => match value.as_ref() {
            Expr::Binary {
                op: BinaryOp::Add,
                left,
                right,
            } => match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(l), _) if *l == counter_id => Some(CounterStep::Add(right)),
                (_, Expr::LocalGet(r)) if *r == counter_id => Some(CounterStep::Add(left)),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// True when `expr` provably evaluates to a **non-negative integral** Number
/// (or to `+Infinity`, which the loop guard rejects before the body runs).
///
/// IEEE-754 `+`/`*` of non-negative integral doubles is again non-negative and
/// integral: below `2^53` the result is exact, and at or above it the whole
/// representable grid is integral. Neither can produce a negative or
/// fractional value, so the induction's lower bound survives every rounding.
/// Overflow saturates to `+Infinity`, and `Infinity < bound` is false, so an
/// overflowed counter never enters the body the fact is attached to.
fn is_nonnegative_integer_valued<F: CounterRangeFacts + ?Sized>(facts: &F, expr: &Expr) -> bool {
    match expr {
        Expr::Integer(n) => *n >= 0,
        Expr::Number(n) => n.is_finite() && n.fract() == 0.0 && *n >= 0.0,
        Expr::LocalGet(id) => {
            facts.is_nonnegative_integer_local(*id)
                || facts.int_range(expr).is_some_and(|range| range.min >= 0)
        }
        Expr::Binary {
            op: BinaryOp::Add | BinaryOp::Mul,
            left,
            right,
        } => {
            is_nonnegative_integer_valued(facts, left)
                && is_nonnegative_integer_valued(facts, right)
        }
        _ => facts.int_range(expr).is_some_and(|range| range.min >= 0),
    }
}

fn collect_local_gets(expr: &Expr, out: &mut Vec<u32>) {
    if let Expr::LocalGet(id) = expr {
        out.push(*id);
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| collect_local_gets(child, out));
}

/// True when every local read by `expr` keeps its value for the whole loop:
/// not written by the guard, the update slot or the body, and not reachable
/// through a closure capture (which could be written by a callee).
fn operands_are_loop_invariant<F: CounterRangeFacts + ?Sized>(
    facts: &F,
    expr: &Expr,
    cond: &Expr,
    update: Option<&Expr>,
    body: &[Stmt],
) -> bool {
    let mut ids = Vec::new();
    collect_local_gets(expr, &mut ids);
    ids.iter().all(|id| {
        !facts.is_closure_captured(*id)
            && !expr_mutates_local(cond, *id)
            && update.is_none_or(|expr| !expr_mutates_local(expr, *id))
            && !stmts_mutate_local(body, *id)
    })
}

/// Prove `[min, max]` for a `for`-loop counter over the whole loop body.
///
/// Returns `None` — the correct answer, not a failure — whenever any part of
/// the proof is missing; the access then keeps the runtime-key helper.
pub(crate) fn classify_for_counter_range<F: CounterRangeFacts + ?Sized>(
    init: Option<&Stmt>,
    cond: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
    facts: &F,
    scope_id: u32,
) -> Option<IntRangeFact> {
    let (counter_id, start) = match init? {
        Stmt::Let {
            id,
            init: Some(start),
            ..
        } => (*id, start),
        _ => return None,
    };
    let cond = cond?;
    let Expr::Compare { op, left, right } = cond else {
        return None;
    };
    if !matches!(op, CompareOp::Lt | CompareOp::Le) {
        return None;
    }
    if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == counter_id) {
        return None;
    }
    let step = counter_step(update?, counter_id)?;

    // The update slot must be the counter's ONLY writer. The fact is attached
    // to the whole body, so a body write invalidates it from the second
    // iteration on: `for (let i = 0; i < 10; i++) { a[i]; i = -5; }` re-enters
    // the body with `i == -4` while the fact still claims `[0, 9]`.
    if expr_mutates_local(cond, counter_id) || stmts_mutate_local(body, counter_id) {
        return None;
    }

    // Upper bound from the guard. Only an exactly-known bound is admitted, so
    // the value the guard compares against is the same one the fact quotes.
    if let Expr::LocalGet(bound_id) = right.as_ref() {
        if expr_mutates_local(cond, *bound_id)
            || update.is_some_and(|expr| expr_mutates_local(expr, *bound_id))
            || stmts_mutate_local(body, *bound_id)
        {
            return None;
        }
    }
    let bound_range = facts.int_range(right)?;
    if bound_range.min != bound_range.max {
        return None;
    }
    let upper = bound_range
        .max
        .checked_sub(if matches!(op, CompareOp::Lt) { 1 } else { 0 })?;

    // A non-literal stride is re-read every iteration, so the compile-time
    // "non-negative integer" fact has to still hold on iteration N.
    if let CounterStep::Add(stride) = step {
        if !is_nonnegative_integer_valued(facts, stride)
            || !operands_are_loop_invariant(facts, stride, cond, update, body)
        {
            return None;
        }
    }

    // Lower bound: monotone steps keep the counter at or above its initial
    // value. A provable start range pins the exact minimum; otherwise a
    // provably non-negative start still gives `>= 0`, which is the half of
    // the proof array indexing actually needs.
    let lower = if let Some(range) = facts.int_range(start) {
        range.min
    } else if is_nonnegative_integer_valued(facts, start) {
        0
    } else {
        return None;
    };

    (lower <= upper).then_some(IntRangeFact {
        local_id: counter_id,
        scope_id,
        range: IntRange {
            min: lower,
            max: upper,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[derive(Default)]
    struct TestFacts {
        nonnegative: HashSet<u32>,
        constants: HashMap<u32, i64>,
        captured: HashSet<u32>,
    }

    impl CounterRangeFacts for TestFacts {
        fn int_range(&self, expr: &Expr) -> Option<IntRange> {
            match expr {
                Expr::Integer(n) => Some(IntRange::exact(*n)),
                Expr::LocalGet(id) => self.constants.get(id).copied().map(IntRange::exact),
                _ => None,
            }
        }

        fn is_nonnegative_integer_local(&self, id: u32) -> bool {
            self.nonnegative.contains(&id)
        }

        fn is_closure_captured(&self, id: u32) -> bool {
            self.captured.contains(&id)
        }
    }

    const COUNTER: u32 = 7;
    const STRIDE: u32 = 3;
    const LIMIT: u32 = 9;

    fn add(left: Expr, right: Expr) -> Expr {
        Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn mul(left: Expr, right: Expr) -> Expr {
        Expr::Binary {
            op: BinaryOp::Mul,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn let_stmt(id: u32, init: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: format!("v{id}"),
            ty: perry_hir::types::Type::Number,
            mutable: true,
            init: Some(init),
        }
    }

    fn lt(left: Expr, right: Expr) -> Expr {
        Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    /// `j = j + <stride>`
    fn strided_update(stride: Expr) -> Expr {
        Expr::LocalSet(
            COUNTER,
            Box::new(add(Expr::LocalGet(COUNTER), stride)),
        )
    }

    fn sieve_facts() -> TestFacts {
        TestFacts {
            nonnegative: HashSet::from([STRIDE]),
            constants: HashMap::from([(LIMIT, 1_000_000)]),
            captured: HashSet::new(),
        }
    }

    /// Lever (a): `for (let j = i * i; j < LIMIT; j = j + i)` — the whole
    /// `11_prime_sieve` inner loop. Neither `i * i` nor `i` has a range, only
    /// non-negativity, and the upper bound comes from the guard.
    #[test]
    fn strided_counter_with_nonnegative_start_and_stride_is_bounded() {
        let init = let_stmt(COUNTER, mul(Expr::LocalGet(STRIDE), Expr::LocalGet(STRIDE)));
        let cond = lt(Expr::LocalGet(COUNTER), Expr::LocalGet(LIMIT));
        let update = strided_update(Expr::LocalGet(STRIDE));
        let fact = classify_for_counter_range(
            Some(&init),
            Some(&cond),
            Some(&update),
            &[],
            &sieve_facts(),
            42,
        )
        .expect("monotone strided counter should be bounded");
        assert_eq!(fact.local_id, COUNTER);
        assert_eq!(fact.scope_id, 42);
        assert_eq!(fact.range.min, 0);
        assert_eq!(fact.range.max, 999_999);
    }

    /// A stride with no non-negativity proof could walk the counter down past
    /// zero (`j = j + step` with `step === -1` visits `-1`, which is the
    /// property `"-1"`, not element 0).
    #[test]
    fn unproven_stride_is_rejected() {
        let init = let_stmt(COUNTER, Expr::Integer(0));
        let cond = lt(Expr::LocalGet(COUNTER), Expr::LocalGet(LIMIT));
        let update = strided_update(Expr::LocalGet(STRIDE));
        let mut facts = sieve_facts();
        facts.nonnegative.remove(&STRIDE);
        assert!(classify_for_counter_range(
            Some(&init),
            Some(&cond),
            Some(&update),
            &[],
            &facts,
            0
        )
        .is_none());
    }

    /// The stride is re-read every iteration: a body that rewrites it voids
    /// the compile-time non-negativity fact.
    #[test]
    fn stride_mutated_by_body_is_rejected() {
        let init = let_stmt(COUNTER, Expr::Integer(0));
        let cond = lt(Expr::LocalGet(COUNTER), Expr::LocalGet(LIMIT));
        let update = strided_update(Expr::LocalGet(STRIDE));
        let body = vec![Stmt::Expr(Expr::LocalSet(
            STRIDE,
            Box::new(Expr::Integer(-1)),
        ))];
        assert!(classify_for_counter_range(
            Some(&init),
            Some(&cond),
            Some(&update),
            &body,
            &sieve_facts(),
            0
        )
        .is_none());
    }

    /// A captured stride can be written by a closure declared outside the loop
    /// and merely called inside it, which the syntactic walk cannot see.
    #[test]
    fn closure_captured_stride_is_rejected() {
        let init = let_stmt(COUNTER, Expr::Integer(0));
        let cond = lt(Expr::LocalGet(COUNTER), Expr::LocalGet(LIMIT));
        let update = strided_update(Expr::LocalGet(STRIDE));
        let mut facts = sieve_facts();
        facts.captured.insert(STRIDE);
        assert!(classify_for_counter_range(
            Some(&init),
            Some(&cond),
            Some(&update),
            &[],
            &facts,
            0
        )
        .is_none());
    }

    /// `for (let i = 0; i < 10; i++) { a[i]; i = -5; }` keeps looping with a
    /// negative `i`. The fact must not survive a body write to the counter.
    #[test]
    fn counter_written_by_body_is_rejected() {
        let init = let_stmt(COUNTER, Expr::Integer(0));
        let cond = lt(Expr::LocalGet(COUNTER), Expr::Integer(10));
        let update = Expr::Update {
            id: COUNTER,
            op: UpdateOp::Increment,
            prefix: false,
        };
        let body = vec![Stmt::Expr(Expr::LocalSet(
            COUNTER,
            Box::new(Expr::Integer(-5)),
        ))];
        assert!(classify_for_counter_range(
            Some(&init),
            Some(&cond),
            Some(&update),
            &body,
            &TestFacts::default(),
            0
        )
        .is_none());
    }

    /// The classic `i++` shape keeps its exact `[start, bound - 1]` fact.
    #[test]
    fn increment_counter_keeps_exact_range() {
        let init = let_stmt(COUNTER, Expr::Integer(2));
        let cond = lt(Expr::LocalGet(COUNTER), Expr::Integer(10));
        let update = Expr::Update {
            id: COUNTER,
            op: UpdateOp::Increment,
            prefix: false,
        };
        let fact = classify_for_counter_range(
            Some(&init),
            Some(&cond),
            Some(&update),
            &[],
            &TestFacts::default(),
            0,
        )
        .expect("literal-start increment loop keeps its fact");
        assert_eq!(fact.range.min, 2);
        assert_eq!(fact.range.max, 9);
    }

    /// A decrementing counter has no lower bound from the guard, so no fact.
    #[test]
    fn decrementing_counter_is_rejected() {
        let init = let_stmt(COUNTER, Expr::Integer(10));
        let cond = lt(Expr::LocalGet(COUNTER), Expr::Integer(100));
        let update = Expr::Update {
            id: COUNTER,
            op: UpdateOp::Decrement,
            prefix: false,
        };
        assert!(classify_for_counter_range(
            Some(&init),
            Some(&cond),
            Some(&update),
            &[],
            &TestFacts::default(),
            0
        )
        .is_none());
    }

    /// A non-constant guard bound (`j < n` for unknown `n`) proves nothing
    /// about the upper end.
    #[test]
    fn unknown_bound_is_rejected() {
        let init = let_stmt(COUNTER, Expr::Integer(0));
        let cond = lt(Expr::LocalGet(COUNTER), Expr::LocalGet(LIMIT));
        let update = strided_update(Expr::Integer(1));
        let mut facts = sieve_facts();
        facts.constants.remove(&LIMIT);
        assert!(classify_for_counter_range(
            Some(&init),
            Some(&cond),
            Some(&update),
            &[],
            &facts,
            0
        )
        .is_none());
    }

    /// A fractional start would make every iterate fractional, and `a[0.5]`
    /// is the property `"0.5"` — not element 0.
    #[test]
    fn fractional_start_is_rejected() {
        let init = let_stmt(COUNTER, Expr::Number(0.5));
        let cond = lt(Expr::LocalGet(COUNTER), Expr::LocalGet(LIMIT));
        let update = strided_update(Expr::LocalGet(STRIDE));
        assert!(classify_for_counter_range(
            Some(&init),
            Some(&cond),
            Some(&update),
            &[],
            &sieve_facts(),
            0
        )
        .is_none());
    }

    /// `j = j - i` is not an `Add`, so it never reaches the stride rules.
    #[test]
    fn subtracting_update_is_rejected() {
        let init = let_stmt(COUNTER, Expr::Integer(0));
        let cond = lt(Expr::LocalGet(COUNTER), Expr::LocalGet(LIMIT));
        let update = Expr::LocalSet(
            COUNTER,
            Box::new(Expr::Binary {
                op: BinaryOp::Sub,
                left: Box::new(Expr::LocalGet(COUNTER)),
                right: Box::new(Expr::LocalGet(STRIDE)),
            }),
        );
        assert!(classify_for_counter_range(
            Some(&init),
            Some(&cond),
            Some(&update),
            &[],
            &sieve_facts(),
            0
        )
        .is_none());
    }

    /// An update that assigns some *other* local is not a step at all.
    #[test]
    fn update_of_other_local_is_rejected() {
        let init = let_stmt(COUNTER, Expr::Integer(0));
        let cond = lt(Expr::LocalGet(COUNTER), Expr::LocalGet(LIMIT));
        let update = Expr::LocalSet(
            STRIDE,
            Box::new(add(Expr::LocalGet(STRIDE), Expr::Integer(1))),
        );
        assert!(classify_for_counter_range(
            Some(&init),
            Some(&cond),
            Some(&update),
            &[],
            &sieve_facts(),
            0
        )
        .is_none());
    }
}
