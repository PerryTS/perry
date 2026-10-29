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
}

impl KnownRanges {
    pub(crate) fn collect(stmts: &[Stmt]) -> Self {
        let facts = super::loop_bounded_i32::collect_loop_induction_facts(stmts, &HashMap::new());
        Self {
            intervals: facts.intervals,
            constants: facts.integer_constants,
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
        Range::I32
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
/// bound is `[0, i32::MAX]`.
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
    None
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
        assert_eq!(for_step_fits(Some(&lt), &upd, &[], &mut r), Some(1));
        assert_eq!(for_step_fits(Some(&le), &upd, &[], &mut r), None);
        let small = Expr::Compare {
            op: CompareOp::Le,
            left: lg(1),
            right: Box::new(Expr::Integer(100)),
        };
        assert_eq!(for_step_fits(Some(&small), &upd, &[], &mut r), Some(1));
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
        assert_eq!(for_step_fits(Some(&le), &upd, &[], &mut r), None);
    }
}
