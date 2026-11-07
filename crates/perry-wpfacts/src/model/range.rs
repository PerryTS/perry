//! Integer intervals of the Number values a node holds (design §10 slice 3:
//! "i32 only with a range proof"; RFC representation-selection §5.2).
//!
//! A node's [`Iv`] bounds the Number values among everything it holds:
//! `Int { lo, hi }` says every such Number is an integer (never `-0`, never
//! NaN or an infinity) in `[lo, hi]`; [`Iv::TOP`] says nothing. What the node
//! holds besides Numbers (`prim`, tokens) is the node's other lattice and is
//! read beside the interval by every consumer.
//!
//! Fail closed: a Number that reaches a node any other way than a value-copy
//! edge, a literal or a [`Cons::Range`](super::Cons::Range) rule makes its
//! interval ⊤ (`Model::add_tokens`). A node that is a rule's result
//! (`Node::ruled`) takes its interval from that rule only: the rule's result
//! is every Number the node can hold. An interval that keeps growing is
//! widened to ⊤ ([`WIDEN_STEPS`]), so cycles cost a bounded number of
//! re-deliveries.

/// Bounds beyond which interval arithmetic stops being exact for doubles:
/// a bound past ±2^53 widens to ⊤.
const SAFE: i64 = 1 << 53;

/// Growths of one node's interval before it widens to ⊤.
pub const WIDEN_STEPS: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Iv {
    pub lo: i64,
    pub hi: i64,
}

impl Default for Iv {
    fn default() -> Self {
        Iv::EMPTY
    }
}

impl Iv {
    /// No Number (the bottom; `lo > hi`).
    pub const EMPTY: Iv = Iv { lo: 1, hi: 0 };
    /// Any Number.
    pub const TOP: Iv = Iv {
        lo: i64::MIN,
        hi: i64::MAX,
    };
    pub const I32: Iv = Iv {
        lo: i32::MIN as i64,
        hi: i32::MAX as i64,
    };
    pub const U32: Iv = Iv {
        lo: 0,
        hi: u32::MAX as i64,
    };

    pub fn is_empty(self) -> bool {
        self.lo > self.hi
    }

    pub fn is_top(self) -> bool {
        self == Iv::TOP
    }

    /// An integer interval with checked bounds; ⊤ past ±2^53.
    pub fn int(lo: i64, hi: i64) -> Iv {
        if lo > hi {
            return Iv::EMPTY;
        }
        if lo < -SAFE || hi > SAFE {
            return Iv::TOP;
        }
        Iv { lo, hi }
    }

    /// The interval of a numeric literal: a point for an integer that is
    /// not `-0`, ⊤ otherwise.
    pub fn literal(v: f64) -> Iv {
        if v.is_finite() && v.fract() == 0.0 && !(v == 0.0 && v.is_sign_negative()) {
            if v.abs() <= SAFE as f64 {
                return Iv::int(v as i64, v as i64);
            }
        }
        Iv::TOP
    }

    pub fn join(self, o: Iv) -> Iv {
        if self.is_empty() {
            return o;
        }
        if o.is_empty() {
            return self;
        }
        if self.is_top() || o.is_top() {
            return Iv::TOP;
        }
        Iv {
            lo: self.lo.min(o.lo),
            hi: self.hi.max(o.hi),
        }
    }

    /// Every value of `self` lies in `o`.
    pub fn within(self, o: Iv) -> bool {
        self.is_empty() || (!self.is_top() && o.lo <= self.lo && self.hi <= o.hi)
    }

    fn has_zero(self) -> bool {
        self.lo <= 0 && 0 <= self.hi
    }
}

/// The operations [`Cons::Range`](super::Cons::Range) evaluates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RangeOp {
    Add,
    Sub,
    Mul,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
    Neg,
    BitNot,
    /// `++` / `--`: the update's value node carries both the old and the
    /// new value (prefix and postfix), so it is their hull.
    Step(i8),
}

impl RangeOp {
    pub fn name(self) -> &'static str {
        match self {
            RangeOp::Add => "add",
            RangeOp::Sub => "sub",
            RangeOp::Mul => "mul",
            RangeOp::Mod => "mod",
            RangeOp::BitAnd => "and",
            RangeOp::BitOr => "or",
            RangeOp::BitXor => "xor",
            RangeOp::Shl => "shl",
            RangeOp::Shr => "shr",
            RangeOp::UShr => "ushr",
            RangeOp::Neg => "neg",
            RangeOp::BitNot => "bitnot",
            RangeOp::Step(1) => "inc",
            RangeOp::Step(_) => "dec",
        }
    }

    /// Whether the Number result is an int32 (or uint32) whatever the
    /// operands hold: ToInt32/ToUint32 applies to every operand.
    pub fn wraps(self) -> bool {
        matches!(
            self,
            RangeOp::BitAnd
                | RangeOp::BitOr
                | RangeOp::BitXor
                | RangeOp::Shl
                | RangeOp::Shr
                | RangeOp::UShr
                | RangeOp::BitNot
        )
    }
}

/// The ToInt32 image of an operand interval: itself when it already lies in
/// the int32 range, else the whole int32 range.
fn to_int32(a: Iv) -> Iv {
    if a.within(Iv::I32) && !a.is_empty() {
        a
    } else {
        Iv::I32
    }
}

/// The smallest `2^k - 1` at or above `v` (`v >= 0`).
fn mask_above(v: i64) -> i64 {
    let mut m: i64 = 0;
    while m < v {
        m = (m << 1) | 1;
    }
    m
}

/// The interval of `op` over operand intervals. `l` and `r` are ⊤ unless the
/// operand holds only Numbers (the caller's purity test): an arithmetic
/// operator then coerces nothing. A unary operator ignores `r`.
pub fn eval(op: RangeOp, l: Iv, r: Iv) -> Iv {
    if l.is_empty()
        || (!matches!(op, RangeOp::Neg | RangeOp::BitNot | RangeOp::Step(_)) && r.is_empty())
    {
        return Iv::EMPTY;
    }
    match op {
        RangeOp::Add if !l.is_top() && !r.is_top() => Iv::int(l.lo + r.lo, l.hi + r.hi),
        RangeOp::Sub if !l.is_top() && !r.is_top() => Iv::int(l.lo - r.hi, l.hi - r.lo),
        RangeOp::Mul if !l.is_top() && !r.is_top() => {
            // `0 * negative` is -0.
            if (l.has_zero() && r.lo < 0) || (r.has_zero() && l.lo < 0) {
                return Iv::TOP;
            }
            let ps = [
                l.lo.checked_mul(r.lo),
                l.lo.checked_mul(r.hi),
                l.hi.checked_mul(r.lo),
                l.hi.checked_mul(r.hi),
            ];
            if ps.iter().any(Option::is_none) {
                return Iv::TOP;
            }
            let ps: Vec<i64> = ps.into_iter().flatten().collect();
            Iv::int(*ps.iter().min().unwrap(), *ps.iter().max().unwrap())
        }
        // A non-negative dividend keeps its sign (never -0); a zero divisor
        // is NaN.
        RangeOp::Mod if !l.is_top() && !r.is_top() && l.lo >= 0 && !r.has_zero() => {
            let m = r.lo.unsigned_abs().max(r.hi.unsigned_abs()) as i64 - 1;
            Iv::int(0, l.hi.min(m))
        }
        RangeOp::Neg if !l.is_top() && !l.has_zero() => Iv::int(-l.hi, -l.lo),
        RangeOp::Step(d) if !l.is_top() => {
            let d = d as i64;
            Iv::int(l.lo.min(l.lo + d), l.hi.max(l.hi + d))
        }
        RangeOp::BitAnd => {
            let (a, b) = (to_int32(l), to_int32(r));
            match (a.lo >= 0, b.lo >= 0) {
                (true, true) => Iv::int(0, a.hi.min(b.hi)),
                (true, false) => Iv::int(0, a.hi),
                (false, true) => Iv::int(0, b.hi),
                (false, false) => Iv::I32,
            }
        }
        RangeOp::BitOr | RangeOp::BitXor => {
            let (a, b) = (to_int32(l), to_int32(r));
            if a.lo >= 0 && b.lo >= 0 {
                Iv::int(0, mask_above(a.hi.max(b.hi)))
            } else {
                Iv::I32
            }
        }
        RangeOp::Shr => {
            let a = to_int32(l);
            match r {
                Iv { lo, hi } if lo == hi && !r.is_top() => {
                    let k = (lo & 31) as u32;
                    Iv::int(a.lo >> k, a.hi >> k)
                }
                _ => Iv::int(a.lo.min(0), a.hi.max(0)),
            }
        }
        RangeOp::UShr => match r {
            Iv { lo, hi } if lo == hi && !r.is_top() => {
                let k = (lo & 31) as u32;
                let a = to_int32(l);
                if a.lo >= 0 {
                    Iv::int(a.lo >> k, a.hi >> k)
                } else {
                    Iv::int(0, (u32::MAX >> k) as i64)
                }
            }
            _ => Iv::U32,
        },
        RangeOp::Shl => Iv::I32,
        RangeOp::BitNot => {
            let a = to_int32(l);
            Iv::int(-a.hi - 1, -a.lo - 1)
        }
        _ => Iv::TOP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_are_points_except_negative_zero_and_fractions() {
        assert_eq!(Iv::literal(5.0), Iv::int(5, 5));
        assert!(Iv::literal(-0.0).is_top());
        assert!(Iv::literal(0.5).is_top());
        assert!(Iv::literal(f64::NAN).is_top());
        assert!(Iv::literal(1e300).is_top());
    }

    #[test]
    fn arithmetic_follows_the_bounds() {
        assert_eq!(
            eval(RangeOp::Add, Iv::int(0, 7), Iv::int(-17, 17)),
            Iv::int(-17, 24)
        );
        assert_eq!(
            eval(RangeOp::Sub, Iv::int(0, 7), Iv::int(1, 1)),
            Iv::int(-1, 6)
        );
        assert_eq!(
            eval(RangeOp::Mul, Iv::int(0, 7), Iv::int(16, 16)),
            Iv::int(0, 112)
        );
        assert!(eval(RangeOp::Add, Iv::TOP, Iv::int(1, 1)).is_top());
        assert!(eval(RangeOp::Add, Iv::int(0, 1 << 53), Iv::int(1, 1)).is_top());
    }

    #[test]
    fn negative_zero_is_never_an_integer_result() {
        // 0 * -1 and -0 are -0.
        assert!(eval(RangeOp::Mul, Iv::int(0, 3), Iv::int(-1, -1)).is_top());
        assert!(eval(RangeOp::Neg, Iv::int(0, 3), Iv::EMPTY).is_top());
        assert_eq!(
            eval(RangeOp::Neg, Iv::int(1, 3), Iv::EMPTY),
            Iv::int(-3, -1)
        );
        // -4 % 2 is -0: a possibly negative dividend is not proven.
        assert!(eval(RangeOp::Mod, Iv::int(-4, 4), Iv::int(2, 2)).is_top());
        assert_eq!(
            eval(RangeOp::Mod, Iv::int(0, 100), Iv::int(8, 8)),
            Iv::int(0, 7)
        );
        assert!(eval(RangeOp::Mod, Iv::int(0, 100), Iv::int(0, 8)).is_top());
    }

    #[test]
    fn bitwise_results_are_int32_whatever_the_operands() {
        assert_eq!(
            eval(RangeOp::BitAnd, Iv::TOP, Iv::int(127, 127)),
            Iv::int(0, 127)
        );
        assert_eq!(eval(RangeOp::BitAnd, Iv::TOP, Iv::TOP), Iv::I32);
        assert_eq!(
            eval(RangeOp::Shr, Iv::int(0, 16383), Iv::int(7, 7)),
            Iv::int(0, 127)
        );
        assert_eq!(
            eval(RangeOp::Shr, Iv::TOP, Iv::int(4, 4)),
            Iv::int(i32::MIN as i64 >> 4, i32::MAX as i64 >> 4)
        );
        assert_eq!(eval(RangeOp::UShr, Iv::TOP, Iv::int(0, 0)), Iv::U32);
        assert_eq!(
            eval(RangeOp::BitOr, Iv::int(0, 5), Iv::int(0, 9)),
            Iv::int(0, 15)
        );
        assert_eq!(
            eval(RangeOp::BitNot, Iv::int(0, 3), Iv::EMPTY),
            Iv::int(-4, -1)
        );
    }

    #[test]
    fn steps_cover_the_old_and_the_new_value() {
        assert_eq!(
            eval(RangeOp::Step(1), Iv::int(0, 7), Iv::EMPTY),
            Iv::int(0, 8)
        );
        assert_eq!(
            eval(RangeOp::Step(-1), Iv::int(0, 7), Iv::EMPTY),
            Iv::int(-1, 7)
        );
    }
}
