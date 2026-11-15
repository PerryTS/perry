//! Exact-value range proofs for `integer_locals`' strict judgment.
//!
//! An i32 slot stores a JS number as an `i32`. That is exact only when the
//! number itself is an integer in `[-2^31, 2^31 - 1]`: a `+`, `-` or `*` of two
//! int32 values is an integer, but its exact value can leave that range, and
//! the `add`/`sub`/`mul i32` that would compute it into the slot keeps only the
//! low 32 bits. Every reader of the slot then sees a different number than the
//! program computed — a loop guard that should exit keeps running, an index
//! that is past the end lands inside the array.
//!
//! So a write of an arithmetic result is int32-producing only when the exact
//! result is proven inside i32 from ranges of its operands:
//!
//! * [`exact_range`] computes a closed interval for the exact JS value with
//!   checked integer arithmetic. Its leaves are integer literals, integer
//!   constants, loop-induction intervals (`collectors/loop_bounded_i32.rs`),
//!   other int32 candidates (the whole i32 range, unless an interval is
//!   known) and the ToInt32/ToUint32-wrapping operators, whose results are
//!   bounded whatever their operands hold.
//! * [`for_step_fits`] proves a `for` loop's own step (`i++`, `i += s`,
//!   `i = i - s`) from the loop guard: the step runs only after `i < B` held
//!   this iteration, so `i + s <= max(B) - 1 + max(s)`. With `B` an int32 value
//!   or a typed-array length and a unit step that is always inside i32; a
//!   `<=` guard or a wider step needs a tighter `B`.
//!
//! * [`overflow_exit_counter`] admits a `for` counter whose step (or init)
//!   is not provably inside i32 but can only leave it UPWARD, past a guard
//!   `i < B` / `i <= B` whose bound is an int32: there the exact value fails
//!   the guard, so the loop exits, and the counter (scoped to the loop) is
//!   never observed again. `stmt/loops.rs` lowers such a counter's init and
//!   step with overflow-checked arithmetic and leaves the loop on overflow,
//!   so the slot holds the exact value at every point the program reads it.
//!
//! Anything unproven keeps the boxed double, which is always exact.

use std::collections::{HashMap, HashSet};

use perry_hir::{BinaryOp, CompareOp, Expr, LogicalOp, Stmt, UnaryOp, UpdateOp};

use super::loop_bounded_i32::IntInterval;

const I32_MIN: i64 = i32::MIN as i64;
const I32_MAX: i64 = i32::MAX as i64;

/// A closed interval of exact integer values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Range {
    pub(crate) lo: i64,
    pub(crate) hi: i64,
}

impl Range {
    pub(crate) const I32: Range = Range {
        lo: I32_MIN,
        hi: I32_MAX,
    };

    pub(crate) fn fits_i32(self) -> bool {
        self.lo >= I32_MIN && self.hi <= I32_MAX
    }

    fn point(n: i64) -> Range {
        Range { lo: n, hi: n }
    }
}

/// Ranges known before the judgment runs: loop-induction intervals and
/// integer constants of the body being analysed.
#[derive(Default)]
pub(crate) struct KnownRanges {
    pub(crate) intervals: HashMap<u32, IntInterval>,
    pub(crate) constants: HashMap<u32, i64>,
    /// `for` counters that only step upward from an integer literal: inside
    /// the loop, where they are read, they are at least that literal. (An
    /// int32 candidate is at most `i32::MAX` by the candidate judgment.)
    pub(crate) floors: HashMap<u32, i64>,
    /// `for (let i = <literal >= 0>; i * i < B; i++ / i += <literal>)` with
    /// `B` an integer literal or constant and nothing else writing `i`:
    /// wherever the loop reads `i` the guard held, so `i <= isqrt(B)`.
    pub(crate) ceilings: HashMap<u32, i64>,
}

impl KnownRanges {
    pub(crate) fn collect(stmts: &[Stmt]) -> Self {
        let facts = super::loop_bounded_i32::collect_loop_induction_facts(stmts, &HashMap::new());
        let mut floors = HashMap::new();
        collect_counter_floors(stmts, &mut floors);
        let mut ceilings = HashMap::new();
        collect_counter_ceilings(stmts, &facts.integer_constants, &floors, &mut ceilings);
        Self {
            intervals: facts.intervals,
            constants: facts.integer_constants,
            floors,
            ceilings,
        }
    }

    /// The range of an int32 candidate local: its constant or induction
    /// interval when one is known, the whole i32 range otherwise.
    pub(crate) fn local(&self, id: u32) -> Range {
        if let Some(&n) = self.constants.get(&id) {
            return Range::point(n);
        }
        if let Some(iv) = self.intervals.get(&id) {
            return Range {
                lo: iv.lo,
                hi: iv.hi,
            };
        }
        if let Some(&lo) = self.floors.get(&id) {
            let hi = self.ceilings.get(&id).copied().unwrap_or(I32_MAX);
            return Range { lo, hi };
        }
        Range::I32
    }
}

/// `for (let i = <literal>; …; <upward step by a literal or by 1>)` with the
/// update the only write: `i >= <literal>` wherever the loop reads it.
fn collect_counter_floors(stmts: &[Stmt], out: &mut HashMap<u32, i64>) {
    for s in stmts {
        match s {
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let (
                    Some(Stmt::Let {
                        id,
                        init: Some(start),
                        ..
                    }),
                    Some(upd),
                ) = (init.as_deref(), update.as_ref())
                {
                    let literal_up = match for_step(upd) {
                        Some((c, Step::Up(StepBy::One))) => c == *id,
                        Some((c, Step::Up(StepBy::Expr(e)))) => {
                            c == *id && integer_constant(e).is_some_and(|k| k >= 0)
                        }
                        _ => false,
                    };
                    let mut written = HashSet::new();
                    super::loop_bounded_i32::collect_written_locals(body, &mut written);
                    if let Some(c) = condition {
                        super::loop_bounded_i32::collect_written_locals(
                            &[Stmt::Expr(c.clone())],
                            &mut written,
                        );
                    }
                    if let (true, Some(n), false) =
                        (literal_up, integer_constant(start), written.contains(id))
                    {
                        out.insert(*id, n);
                    }
                }
                collect_counter_floors(body, out);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_counter_floors(then_branch, out);
                if let Some(e) = else_branch {
                    collect_counter_floors(e, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_counter_floors(body, out)
            }
            Stmt::Labeled { body, .. } => {
                collect_counter_floors(std::slice::from_ref(body.as_ref()), out)
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_counter_floors(body, out);
                if let Some(c) = catch {
                    collect_counter_floors(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_counter_floors(f, out);
                }
            }
            _ => {}
        }
    }
}

/// Fill `out` with the ceilings described on [`KnownRanges::ceilings`]. A
/// counter qualifies only when it has a floor (literal init >= 0, literal or
/// unit upward step, update the only write) and its guard is the whole
/// condition `i * i < B` / `i * i <= B` with an int32 literal or constant `B`
/// that the loop does not write.
fn collect_counter_ceilings(
    stmts: &[Stmt],
    constants: &HashMap<u32, i64>,
    floors: &HashMap<u32, i64>,
    out: &mut HashMap<u32, i64>,
) {
    for s in stmts {
        match s {
            Stmt::For {
                init,
                condition,
                body,
                ..
            } => {
                if let (Some(Stmt::Let { id, .. }), Some(cond)) = (init.as_deref(), condition) {
                    if let (
                        true,
                        Expr::Compare {
                            op: op @ (CompareOp::Lt | CompareOp::Le),
                            left,
                            right,
                        },
                    ) = (floors.get(id).is_some_and(|f| *f >= 0), cond)
                    {
                        let squared = matches!(left.as_ref(), Expr::Binary { op: BinaryOp::Mul, left: l, right: r }
                            if matches!((l.as_ref(), r.as_ref()), (Expr::LocalGet(a), Expr::LocalGet(b)) if a == id && b == id));
                        let bound = match right.as_ref() {
                            Expr::Integer(n) => Some(*n),
                            Expr::LocalGet(b) if b != id => constants.get(b).copied(),
                            _ => None,
                        };
                        let mut written = HashSet::new();
                        super::loop_bounded_i32::collect_written_locals(body, &mut written);
                        if let (true, Some(b), false) = (
                            squared,
                            bound.filter(|b| (I32_MIN..=I32_MAX).contains(b)),
                            written.contains(id),
                        ) {
                            if let Some(root) = max_root(b, matches!(op, CompareOp::Lt)) {
                                out.insert(*id, root);
                            }
                        }
                    }
                }
                collect_counter_ceilings(body, constants, floors, out);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_counter_ceilings(then_branch, constants, floors, out);
                if let Some(e) = else_branch {
                    collect_counter_ceilings(e, constants, floors, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_counter_ceilings(body, constants, floors, out)
            }
            Stmt::Labeled { body, .. } => collect_counter_ceilings(
                std::slice::from_ref(body.as_ref()),
                constants,
                floors,
                out,
            ),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_counter_ceilings(body, constants, floors, out);
                if let Some(c) = catch {
                    collect_counter_ceilings(&c.body, constants, floors, out);
                }
                if let Some(f) = finally {
                    collect_counter_ceilings(f, constants, floors, out);
                }
            }
            _ => {}
        }
    }
}

fn integer_constant(e: &Expr) -> Option<i64> {
    match e {
        Expr::Integer(n) => Some(*n),
        Expr::Number(n) if n.is_finite() && n.fract() == 0.0 && n.abs() <= 2f64.powi(53) => {
            Some(*n as i64)
        }
        _ => None,
    }
}

fn combine(op: BinaryOp, l: Range, r: Range) -> Option<Range> {
    let (l_lo, l_hi, r_lo, r_hi) = (l.lo as i128, l.hi as i128, r.lo as i128, r.hi as i128);
    let (lo, hi) = match op {
        BinaryOp::Add => (l_lo + r_lo, l_hi + r_hi),
        BinaryOp::Sub => (l_lo - r_hi, l_hi - r_lo),
        BinaryOp::Mul => {
            let corners = [l_lo * r_lo, l_lo * r_hi, l_hi * r_lo, l_hi * r_hi];
            (
                *corners.iter().min().expect("four corners"),
                *corners.iter().max().expect("four corners"),
            )
        }
        _ => return None,
    };
    Some(Range {
        lo: i64::try_from(lo).ok()?,
        hi: i64::try_from(hi).ok()?,
    })
}

/// Interval of the exact JS value of `e`, or `None` when it is not proven an
/// integer in a bounded range.
///
/// `leaf` judges every subexpression that is neither a literal, a candidate
/// local nor `+`/`-`/`*`: it returns that subexpression's int32 range when the
/// caller's int32 judgment accepts it (recording its deps), `None` otherwise.
/// `local` returns a candidate's range and records the dependency.
pub(crate) fn exact_range(
    e: &Expr,
    local: &mut dyn FnMut(u32) -> Option<Range>,
    leaf: &mut dyn FnMut(&Expr) -> Option<Range>,
) -> Option<Range> {
    if let Some(n) = integer_constant(e) {
        return Some(Range::point(n));
    }
    match e {
        Expr::LocalGet(id) => local(*id),
        Expr::Binary { op, left, right }
            if matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul) =>
        {
            let l = exact_range(left, local, leaf)?;
            let r = exact_range(right, local, leaf)?;
            combine(*op, l, r)
        }
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => {
            let r = exact_range(operand, local, leaf)?;
            Some(Range {
                lo: r.hi.checked_neg()?,
                hi: r.lo.checked_neg()?,
            })
        }
        _ => leaf(e),
    }
}

/// Range of an operator whose result is wrapped by ToInt32/ToUint32, i.e.
/// bounded for any operand: `x >>> k` for a literal `k` in `1..32` and
/// `x & m` for a non-negative literal mask. `None` for every other shape.
pub(crate) fn wrapped_operator_range(e: &Expr) -> Option<Range> {
    let Expr::Binary { op, left, right } = e else {
        return None;
    };
    match op {
        BinaryOp::UShr => match integer_constant(right) {
            Some(k) if (1..32).contains(&k) => Some(Range {
                lo: 0,
                hi: (1i64 << (32 - k)) - 1,
            }),
            _ => None,
        },
        BinaryOp::BitAnd => {
            let mask = integer_constant(left)
                .or_else(|| integer_constant(right))
                .filter(|m| (0..=I32_MAX).contains(m))?;
            Some(Range { lo: 0, hi: mask })
        }
        _ => None,
    }
}

/// How a `for` update advances its counter.
enum Step<'a> {
    Up(StepBy<'a>),
    Down(StepBy<'a>),
}

enum StepBy<'a> {
    One,
    Expr(&'a Expr),
}

/// The counter a `for` update steps, and how: `i++`/`i--`/`++i`/`--i`,
/// `i = i + s`, `i = s + i`, `i = i - s` (`i += s` / `i -= s` lower to these).
fn for_step(update: &Expr) -> Option<(u32, Step<'_>)> {
    match update {
        Expr::Update { id, op, .. } => Some((
            *id,
            match op {
                UpdateOp::Increment => Step::Up(StepBy::One),
                UpdateOp::Decrement => Step::Down(StepBy::One),
            },
        )),
        Expr::LocalSet(id, value) => match value.as_ref() {
            Expr::Binary {
                op: BinaryOp::Add,
                left,
                right,
            } => match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(l), s) if l == id => Some((*id, Step::Up(StepBy::Expr(s)))),
                (s, Expr::LocalGet(r)) if r == id => Some((*id, Step::Up(StepBy::Expr(s)))),
                _ => None,
            },
            Expr::Binary {
                op: BinaryOp::Sub,
                left,
                right,
            } => match left.as_ref() {
                Expr::LocalGet(l) if l == id => Some((*id, Step::Down(StepBy::Expr(right)))),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// Every `counter <op> bound` conjunct on the top-level `&&` spine of a loop
/// guard, normalised so the counter is on the left.
fn guard_conjuncts(cond: &Expr, counter: u32, out: &mut Vec<(CompareOp, Expr)>) {
    match cond {
        Expr::Logical {
            op: LogicalOp::And,
            left,
            right,
        } => {
            guard_conjuncts(left, counter, out);
            guard_conjuncts(right, counter, out);
        }
        Expr::Compare { op, left, right } => match (left.as_ref(), right.as_ref()) {
            (Expr::LocalGet(id), bound) if *id == counter => out.push((*op, bound.clone())),
            (bound, Expr::LocalGet(id)) if *id == counter => {
                let flipped = match op {
                    CompareOp::Lt => CompareOp::Gt,
                    CompareOp::Le => CompareOp::Ge,
                    CompareOp::Gt => CompareOp::Lt,
                    CompareOp::Ge => CompareOp::Le,
                    _ => return,
                };
                out.push((flipped, bound.clone()));
            }
            _ => {}
        },
        _ => {}
    }
}

fn local_gets(e: &Expr, out: &mut Vec<u32>) {
    if let Expr::LocalGet(id) = e {
        out.push(*id);
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| local_gets(c, out));
}

/// Range of a loop-guard bound. A `.length` read is a typed-array, buffer or
/// array length; perry's i32 loop lowering already materialises those as i32
/// (`stmt/loops.rs` hoists `arr.length` into an `icmp slt i32` bound), so the
/// bound is `[0, i32::MAX]`. That is a model assumption shared with the hoist:
/// typed arrays and buffers cannot be longer, but a sparse JS array whose
/// `length` is set past `2^31 - 1` is outside it for both.
fn bound_range(bound: &Expr, range_of: &mut dyn FnMut(&Expr) -> Option<Range>) -> Option<Range> {
    match bound {
        Expr::PropertyGet { property, .. } if property == "length" => {
            Some(Range { lo: 0, hi: I32_MAX })
        }
        _ => range_of(bound),
    }
}

/// True when a `for` loop's own update keeps its counter inside i32: the
/// update is a step in one direction, a guard conjunct bounds the counter in
/// that direction by a loop-invariant bound, nothing in the body writes the
/// counter, and `bound ± step` is proven inside i32 (see the module comment).
///
/// `range_of` is the caller's [`exact_range`] (recording deps).
pub(crate) fn for_step_fits(
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: &Expr,
    body: &[Stmt],
    range_of: &mut dyn FnMut(&Expr) -> Option<Range>,
) -> Option<u32> {
    let (counter, step) = for_step(update)?;
    let cond = condition?;
    let mut written = HashSet::new();
    super::loop_bounded_i32::collect_written_locals(body, &mut written);
    if written.contains(&counter) {
        return None;
    }
    // The update writes only the counter; a bound read anywhere written in the
    // loop would not be invariant.
    let invariant = |e: &Expr| {
        let mut ids = Vec::new();
        local_gets(e, &mut ids);
        ids.iter().all(|id| *id != counter && !written.contains(id))
    };
    let step_range = |by: &StepBy<'_>, range_of: &mut dyn FnMut(&Expr) -> Option<Range>| match by {
        StepBy::One => Some(Range::point(1)),
        StepBy::Expr(s) if invariant(s) => range_of(s).filter(|r| r.lo >= 0),
        StepBy::Expr(_) => None,
    };
    let mut conjuncts = Vec::new();
    guard_conjuncts(cond, counter, &mut conjuncts);
    for (op, bound) in &conjuncts {
        if !invariant(bound) {
            continue;
        }
        let proven = match (&step, op) {
            (Step::Up(by), CompareOp::Lt | CompareOp::Le) => (|| {
                let b = bound_range(bound, range_of)?;
                let s = step_range(by, range_of)?;
                let before = if matches!(op, CompareOp::Lt) {
                    b.hi - 1
                } else {
                    b.hi
                };
                before.checked_add(s.hi).map(|after| after <= I32_MAX)
            })(),
            (Step::Down(by), CompareOp::Gt | CompareOp::Ge) => (|| {
                let b = bound_range(bound, range_of)?;
                let s = step_range(by, range_of)?;
                let before = if matches!(op, CompareOp::Gt) {
                    b.lo + 1
                } else {
                    b.lo
                };
                before.checked_sub(s.hi).map(|after| after >= I32_MIN)
            })(),
            _ => None,
        };
        if proven == Some(true) {
            return Some(counter);
        }
    }
    squared_guard_fits(init, cond, counter, &step, &written, range_of)
}

/// The largest `c >= 0` with `c * c < b` (`strict`) or `c * c <= b`, for an
/// int32 `b`; `None` when no such `c` exists. Exact in i64.
fn max_root(b: i64, strict: bool) -> Option<i64> {
    let limit = if strict { b.checked_sub(1)? } else { b };
    if limit < 0 {
        return None;
    }
    let mut c = (limit as f64).sqrt() as i64;
    while c * c > limit {
        c -= 1;
    }
    while (c + 1) * (c + 1) <= limit {
        c += 1;
    }
    Some(c)
}

/// A counter bounded through its square: `for (let i = I; i * i < B; i += s)`
/// (or `<=`). The guard is the WHOLE condition, `I` is an int32 proven
/// non-negative, `s` is 1 or a non-negative int32 literal, `B` is an int32
/// value (literal, constant or int32 local) the body does not write, and the
/// body does not write the counter. The counter then only grows from `I >= 0`,
/// and a step runs only after `i * i < B <= i32::MAX` held, so `i <= 46340`
/// there and `i + s <= 46340 + s` stays inside i32.
fn squared_guard_fits(
    init: Option<&Stmt>,
    cond: &Expr,
    counter: u32,
    step: &Step<'_>,
    written: &HashSet<u32>,
    range_of: &mut dyn FnMut(&Expr) -> Option<Range>,
) -> Option<u32> {
    let Some(Stmt::Let {
        id,
        init: Some(start),
        ..
    }) = init
    else {
        return None;
    };
    if *id != counter || written.contains(&counter) {
        return None;
    }
    let Step::Up(by) = step else {
        return None;
    };
    let step_max = match by {
        StepBy::One => 1,
        StepBy::Expr(Expr::Integer(n)) if (0..=I32_MAX).contains(n) => *n,
        StepBy::Expr(_) => return None,
    };
    let Expr::Compare { op, left, right } = cond else {
        return None;
    };
    if !matches!(op, CompareOp::Lt | CompareOp::Le) {
        return None;
    }
    let Expr::Binary {
        op: BinaryOp::Mul,
        left: l,
        right: r,
    } = left.as_ref()
    else {
        return None;
    };
    if !matches!((l.as_ref(), r.as_ref()), (Expr::LocalGet(a), Expr::LocalGet(b)) if *a == counter && *b == counter)
    {
        return None;
    }
    let bound_ok = match right.as_ref() {
        Expr::Integer(n) => (I32_MIN..=I32_MAX).contains(n),
        Expr::LocalGet(b) => *b != counter && !written.contains(b),
        _ => false,
    };
    if !bound_ok {
        return None;
    }
    let b = range_of(right)?;
    if !b.fits_i32() {
        return None;
    }
    let s = range_of(start)?;
    if s.lo < 0 || !s.fits_i32() {
        return None;
    }
    let root = max_root(b.hi, matches!(op, CompareOp::Lt))?;
    root.checked_add(step_max)
        .filter(|after| *after <= I32_MAX)
        .map(|_| counter)
}

/// The `counter < B` / `counter <= B` conjuncts of a loop guard (counter on
/// the left after normalising), for the counter-versioned loop's entry check.
pub(crate) fn upper_guard_conjuncts(cond: &Expr, counter: u32) -> Vec<(CompareOp, Expr)> {
    let mut out = Vec::new();
    guard_conjuncts(cond, counter, &mut out);
    out.retain(|(op, _)| matches!(op, CompareOp::Lt | CompareOp::Le));
    out
}

/// An init the overflow-exit lowering can compute exactly: one `+` or `*` of
/// two non-negative int32 leaves (`i * i`), whose exact value is a
/// non-negative i64 — so when it is not an i32 it is above `i32::MAX`.
pub(crate) fn is_nonneg_exit_init(
    e: &Expr,
    leaf_range: &mut dyn FnMut(&Expr) -> Option<Range>,
) -> bool {
    let Expr::Binary {
        op: BinaryOp::Add | BinaryOp::Mul,
        left,
        right,
    } = e
    else {
        return false;
    };
    [left, right].into_iter().all(|leaf| {
        !matches!(leaf.as_ref(), Expr::Binary { .. })
            && leaf_range(leaf).is_some_and(|r| r.lo >= 0 && r.fits_i32())
    })
}

/// A `for` counter that steps UP under a `counter < B` / `counter <= B`
/// conjunct with an int32 `B`, whose step is a non-negative loop-invariant
/// int32 and whose init either fits i32 or is one non-negative `+`/`*`
/// ([`is_nonneg_exit_init`]). Every exact value it can take that leaves i32
/// is above `i32::MAX >= B`, so the guard is false for it: the overflow-exit
/// lowering (`stmt/loops.rs`) leaves the loop instead of storing it.
///
/// Only a counter declared by the loop's own `let` qualifies — it is not
/// observable after the loop — and nothing but the update may write it.
pub(crate) fn overflow_exit_counter(
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: &Expr,
    body: &[Stmt],
    range_of: &mut dyn FnMut(&Expr) -> Option<Range>,
) -> Option<u32> {
    let (counter, step) = for_step(update)?;
    let Some(Stmt::Let {
        id,
        init: Some(start),
        ..
    }) = init
    else {
        return None;
    };
    if *id != counter {
        return None;
    }
    let Step::Up(by) = step else {
        return None;
    };
    let cond = condition?;
    let mut written = HashSet::new();
    super::loop_bounded_i32::collect_written_locals(body, &mut written);
    super::loop_bounded_i32::collect_written_locals(&[Stmt::Expr(cond.clone())], &mut written);
    if written.contains(&counter) {
        return None;
    }
    let invariant = |e: &Expr| {
        let mut ids = Vec::new();
        local_gets(e, &mut ids);
        ids.iter().all(|id| *id != counter && !written.contains(id))
    };
    match by {
        StepBy::One => {}
        StepBy::Expr(s) => {
            if !invariant(s) || !range_of(s).is_some_and(|r| r.lo >= 0 && r.fits_i32()) {
                return None;
            }
        }
    }
    let mut conjuncts = Vec::new();
    guard_conjuncts(cond, counter, &mut conjuncts);
    // A length bound is a generic property read whose value is only
    // known to be an int32 when the loop hoists it as one; the collector
    // cannot see that. A counter with any step but one under such a bound is
    // an int32 counter only where it indexes (the int32 counters of the
    // unchecked model); otherwise it stays a double, the JS value itself.
    let unit_step = matches!(by, StepBy::One)
        || super::index_uses::collect_index_used_locals(body).contains(&counter);
    let guarded = conjuncts.iter().any(|(op, bound)| {
        let length_bound =
            matches!(bound, Expr::PropertyGet { property, .. } if property == "length");
        matches!(op, CompareOp::Lt | CompareOp::Le)
            && invariant(bound)
            && (unit_step || !length_bound)
            && bound_range(bound, range_of).is_some_and(|b| b.hi <= I32_MAX)
    });
    if !guarded {
        return None;
    }
    let init_ok =
        range_of(start).is_some_and(Range::fits_i32) || is_nonneg_exit_init(start, range_of);
    init_ok.then_some(counter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lg(id: u32) -> Box<Expr> {
        Box::new(Expr::LocalGet(id))
    }

    fn bin(op: BinaryOp, l: Box<Expr>, r: Box<Expr>) -> Box<Expr> {
        Box::new(Expr::Binary {
            op,
            left: l,
            right: r,
        })
    }

    fn range(e: &Expr) -> Option<Range> {
        exact_range(e, &mut |_| Some(Range::I32), &mut |_| None)
    }

    #[test]
    fn product_of_two_int32_values_is_not_int32() {
        let sq = bin(BinaryOp::Mul, lg(1), lg(1));
        assert!(!range(&sq).unwrap().fits_i32());
        let sum = bin(BinaryOp::Add, lg(1), lg(2));
        assert!(!range(&sum).unwrap().fits_i32());
        let diff = bin(BinaryOp::Sub, lg(1), lg(2));
        assert!(!range(&diff).unwrap().fits_i32());
    }

    #[test]
    fn bounded_operands_prove_the_result() {
        let e = bin(
            BinaryOp::Add,
            bin(BinaryOp::Mul, lg(1), Box::new(Expr::Integer(3))),
            Box::new(Expr::Integer(2)),
        );
        let r = exact_range(&e, &mut |_| Some(Range { lo: 0, hi: 1000 }), &mut |_| None).unwrap();
        assert_eq!(r, Range { lo: 2, hi: 3002 });
        assert!(r.fits_i32());
    }

    #[test]
    fn unit_step_under_strict_guard_fits_but_le_guard_does_not() {
        let upd = Expr::Update {
            id: 1,
            op: UpdateOp::Increment,
            prefix: false,
        };
        let lt = Expr::Compare {
            op: CompareOp::Lt,
            left: lg(1),
            right: lg(2),
        };
        let le = Expr::Compare {
            op: CompareOp::Le,
            left: lg(1),
            right: lg(2),
        };
        let mut r = |e: &Expr| range(e);
        assert_eq!(for_step_fits(None, Some(&lt), &upd, &[], &mut r), Some(1));
        assert_eq!(for_step_fits(None, Some(&le), &upd, &[], &mut r), None);
        let small = Expr::Compare {
            op: CompareOp::Le,
            left: lg(1),
            right: Box::new(Expr::Integer(100)),
        };
        assert_eq!(
            for_step_fits(None, Some(&small), &upd, &[], &mut r),
            Some(1)
        );
    }

    #[test]
    fn symbolic_stride_is_not_proven() {
        // `for (…; j <= n; j += i)` with int32 `n` and `i`: `n + i` can exceed i32.
        let upd = Expr::LocalSet(1, bin(BinaryOp::Add, lg(1), lg(3)));
        let le = Expr::Compare {
            op: CompareOp::Le,
            left: lg(1),
            right: lg(2),
        };
        let mut r = |e: &Expr| range(e);
        assert_eq!(for_step_fits(None, Some(&le), &upd, &[], &mut r), None);
    }

    fn let_counter(init: Expr) -> Stmt {
        Stmt::Let {
            id: 1,
            name: "i".to_string(),
            ty: perry_hir::types::Type::Any,
            mutable: true,
            init: Some(init),
        }
    }

    fn square_guard(op: CompareOp, bound: Expr) -> Expr {
        Expr::Compare {
            op,
            left: bin(BinaryOp::Mul, lg(1), lg(1)),
            right: Box::new(bound),
        }
    }

    fn incr() -> Expr {
        Expr::Update {
            id: 1,
            op: UpdateOp::Increment,
            prefix: false,
        }
    }

    // Local 2 is an int32; local 3 is not provably one.
    fn square_range(e: &Expr) -> Option<Range> {
        match e {
            Expr::LocalGet(3) => None,
            _ => range(e),
        }
    }

    #[test]
    fn squared_guard_admits_counter() {
        let init = let_counter(Expr::Integer(2));
        let mut r = |e: &Expr| square_range(e);
        for cond in [
            square_guard(CompareOp::Lt, Expr::Integer(1_000_000)),
            square_guard(CompareOp::Le, Expr::Integer(i64::from(i32::MAX))),
            square_guard(CompareOp::Lt, Expr::LocalGet(2)),
        ] {
            assert_eq!(
                for_step_fits(Some(&init), Some(&cond), &incr(), &[], &mut r),
                Some(1)
            );
        }
        let by_ten = Expr::LocalSet(1, bin(BinaryOp::Add, lg(1), Box::new(Expr::Integer(10))));
        let cond = square_guard(CompareOp::Lt, Expr::LocalGet(2));
        assert_eq!(
            for_step_fits(Some(&init), Some(&cond), &by_ten, &[], &mut r),
            Some(1)
        );
        // 46340 + step must stay inside i32.
        let huge = Expr::LocalSet(
            1,
            bin(
                BinaryOp::Add,
                lg(1),
                Box::new(Expr::Integer(i64::from(i32::MAX))),
            ),
        );
        assert_eq!(
            for_step_fits(Some(&init), Some(&cond), &huge, &[], &mut r),
            None
        );
    }

    #[test]
    fn squared_guard_rejects_unproven_shapes() {
        let init = let_counter(Expr::Integer(2));
        let mut r = |e: &Expr| square_range(e);
        let cond = square_guard(CompareOp::Lt, Expr::LocalGet(2));
        // Bound written by the body.
        let writes_bound = [Stmt::Expr(Expr::LocalSet(2, Box::new(Expr::Integer(9))))];
        assert_eq!(
            for_step_fits(Some(&init), Some(&cond), &incr(), &writes_bound, &mut r),
            None
        );
        // Counter written by the body.
        let writes_counter = [Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Integer(0))))];
        assert_eq!(
            for_step_fits(Some(&init), Some(&cond), &incr(), &writes_counter, &mut r),
            None
        );
        // Bound that is not an int32.
        let non_int = square_guard(CompareOp::Lt, Expr::LocalGet(3));
        assert_eq!(
            for_step_fits(Some(&init), Some(&non_int), &incr(), &[], &mut r),
            None
        );
        // Downward step.
        let down = Expr::Update {
            id: 1,
            op: UpdateOp::Decrement,
            prefix: false,
        };
        assert_eq!(
            for_step_fits(Some(&init), Some(&cond), &down, &[], &mut r),
            None
        );
        // Negative init.
        let neg = let_counter(Expr::Integer(-1));
        assert_eq!(
            for_step_fits(Some(&neg), Some(&cond), &incr(), &[], &mut r),
            None
        );
        // Not the whole condition.
        let conj = Expr::Logical {
            op: LogicalOp::And,
            left: Box::new(cond.clone()),
            right: Box::new(Expr::Bool(true)),
        };
        assert_eq!(
            for_step_fits(Some(&init), Some(&conj), &incr(), &[], &mut r),
            None
        );
        // Non-literal step.
        let sym = Expr::LocalSet(1, bin(BinaryOp::Add, lg(1), lg(2)));
        assert_eq!(
            for_step_fits(Some(&init), Some(&cond), &sym, &[], &mut r),
            None
        );
    }

    fn length_bound(op: CompareOp) -> Expr {
        Expr::Compare {
            op,
            left: lg(1),
            right: Box::new(Expr::PropertyGet {
                byte_end: 0,
                byte_offset: 0,
                object: lg(2),
                property: "length".to_string(),
            }),
        }
    }

    #[test]
    fn length_bound_admits_only_a_unit_step_exit_counter() {
        let init = let_counter(Expr::Integer(0));
        let mut r = |e: &Expr| range(e);
        let by_997 = Expr::LocalSet(1, bin(BinaryOp::Add, lg(1), Box::new(Expr::Integer(997))));
        for op in [CompareOp::Lt, CompareOp::Le] {
            let cond = length_bound(op);
            // A step other than one under a length bound stays a double.
            assert_eq!(
                overflow_exit_counter(Some(&init), Some(&cond), &by_997, &[], &mut r),
                None
            );
            // The unit step keeps the exit-checked counter.
            assert_eq!(
                overflow_exit_counter(Some(&init), Some(&cond), &incr(), &[], &mut r),
                Some(1)
            );
        }
        // A counter that indexes keeps the exit-checked counter.
        let indexing = [Stmt::Expr(Expr::IndexGet {
            element_offset: 0,
            object: lg(2),
            index: lg(1),
            byte_offset: 0,
            byte_end: 0,
        })];
        assert_eq!(
            overflow_exit_counter(
                Some(&init),
                Some(&length_bound(CompareOp::Lt)),
                &by_997,
                &indexing,
                &mut r
            ),
            Some(1)
        );
        // The same step under a literal bound is admitted.
        let lit = square_guard_plain(CompareOp::Lt, Expr::Integer(1_000_000));
        assert_eq!(
            overflow_exit_counter(Some(&init), Some(&lit), &by_997, &[], &mut r),
            Some(1)
        );
    }

    fn square_guard_plain(op: CompareOp, bound: Expr) -> Expr {
        Expr::Compare {
            op,
            left: lg(1),
            right: Box::new(bound),
        }
    }

    #[test]
    fn max_root_is_exact() {
        assert_eq!(max_root(i64::from(i32::MAX), false), Some(46340));
        assert_eq!(max_root(i64::from(i32::MAX), true), Some(46340));
        assert_eq!(max_root(46340 * 46340, false), Some(46340));
        assert_eq!(max_root(46340 * 46340, true), Some(46339));
        assert_eq!(max_root(0, true), None);
    }
}
