//! Integer intervals of the Number values a node holds (design §10 slice 3:
//! "i32 only with a range proof"; RFC representation-selection §5.2).
//!
//! A node's [`Iv`] bounds the Number values among everything it holds:
//! `Int { lo, hi }` says every such Number is an integer (never `-0`, never
//! NaN or an infinity) in `[lo, hi]`; [`Iv::TOP`] says nothing. What the node
//! holds besides Numbers (`prim`, tokens) is the node's other lattice and is
//! read beside the interval by every consumer.
//!
//! Past ±2^53 a bound is [`BIG`]: every such Number is still an integer
//! (a finite double that large has no fraction), finite and never `-0`, only
//! not exactly known. `Add`/`Sub` keep that while at most one operand
//! reaches `BIG` (a bounded integer added to any finite double rounds to an
//! integer, never past `Number.MAX_VALUE`); `Mul` of a `BIG` operand may
//! overflow to an infinity: ⊤.
//!
//! Fail closed: a Number that reaches a node any other way than a value-copy
//! edge, a literal or a [`Cons::Range`](super::Cons::Range) rule makes its
//! interval ⊤ (`Model::add_tokens`). A node that is a rule's result
//! (`Node::ruled`) takes its interval from that rule only: the rule's result
//! is every Number the node can hold. An interval that keeps growing is
//! widened to ⊤ ([`WIDEN_STEPS`]), so cycles cost a bounded number of
//! re-deliveries.

/// Bounds beyond which interval arithmetic stops being exact for doubles:
/// a bound past ±2^53 is ±[`BIG`].
const SAFE: i64 = 1 << 53;

/// An unbounded integral bound: past ±2^53, up to `Number.MAX_VALUE`.
pub const BIG: i64 = 1 << 60;

/// Growths of one node's interval before its bounds widen to the next
/// threshold ([`widen`]); past `WIDEN_LIMIT` growths it is ⊤.
pub const WIDEN_STEPS: u8 = 3;
pub const WIDEN_LIMIT: u8 = 80;

/// The thresholds a growing bound jumps through: the representation
/// boundaries a consumer asks about (a byte, a 0x88 board, a code unit, the
/// int32 and uint32 maxima). Few rungs, so a cycle meets few re-deliveries
/// (each growth re-delivers the node); a test bound below a rung stops the
/// climb.
const LADDER: [i64; 8] = [0, 1, 7, 127, 255, 65535, i32::MAX as i64, u32::MAX as i64];

/// The next threshold at or above an upper bound: a [`LADDER`] rung, then
/// 2^53, then [`BIG`].
fn hi_threshold(h: i64) -> i64 {
    if h < 0 {
        -1
    } else if h <= u32::MAX as i64 {
        *LADDER.iter().find(|&&t| t >= h).unwrap()
    } else if h <= SAFE {
        SAFE
    } else {
        BIG
    }
}

fn lo_threshold(l: i64) -> i64 {
    if l >= 0 {
        0
    } else if l >= -(1i64 << 32) {
        -(*LADDER.iter().find(|&&t| t + 1 >= -l).unwrap() + 1)
    } else if l >= -SAFE {
        -SAFE
    } else {
        -BIG
    }
}

/// `next` widened against `old`: each bound that grew jumps to its
/// threshold.
pub fn widen(old: Iv, next: Iv) -> Iv {
    if old.is_empty() || next.is_top() {
        return next;
    }
    let hi = if next.hi > old.hi {
        hi_threshold(next.hi)
    } else {
        next.hi
    };
    let lo = if next.lo < old.lo {
        lo_threshold(next.lo)
    } else {
        next.lo
    };
    Iv::int(lo, hi)
}

/// The two lattices a value-copy delivers beside the values: the Numbers'
/// integer interval and the Strings' code interval (`Node::crange`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rg {
    pub num: Iv,
    pub chr: Iv,
}

impl Rg {
    pub const TOP: Rg = Rg {
        num: Iv::TOP,
        chr: Iv::TOP,
    };
}

/// Every String is `""` (code -1) or one code unit.
pub const CHAR_ANY: Iv = Iv { lo: -1, hi: 65535 };

/// The code interval of a String literal: a point for at most one code
/// unit (`""` is -1), ⊤ for a longer one.
pub fn char_literal(s: &str) -> Iv {
    let mut units = s.encode_utf16();
    match (units.next(), units.next()) {
        (None, _) => Iv { lo: -1, hi: -1 },
        (Some(u), None) => Iv {
            lo: u as i64,
            hi: u as i64,
        },
        _ => Iv::TOP,
    }
}

/// `Number(s)` of Strings in code interval `c`: `""` is 0, a digit its
/// value; any other code may be NaN (⊤).
pub fn chars_to_number(c: Iv) -> Iv {
    if c.is_empty() {
        return Iv::EMPTY;
    }
    if c.is_top() {
        return Iv::TOP;
    }
    if c.lo == -1 && c.hi == -1 {
        return Iv { lo: 0, hi: 0 };
    }
    if c.lo >= 0x30 && c.hi <= 0x39 {
        return Iv {
            lo: c.lo - 0x30,
            hi: c.hi - 0x30,
        };
    }
    Iv::TOP
}

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

    /// An integer interval with checked bounds; a bound past ±2^53 is
    /// ±[`BIG`]. Only for a result that is an integer and finite whatever
    /// its magnitude (`eval` decides).
    pub fn int(lo: i64, hi: i64) -> Iv {
        if lo > hi {
            return Iv::EMPTY;
        }
        let lo = if lo < -SAFE { -BIG } else { lo };
        let hi = if hi > SAFE { BIG } else { hi };
        Iv { lo, hi }
    }

    /// Some bound is past ±2^53.
    pub fn big(self) -> bool {
        !self.is_empty() && !self.is_top() && (self.lo < -SAFE || self.hi > SAFE)
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
    /// The values of the left operand that pass a loop test against the
    /// right one (`CLIP_LT` ...): the reads the test dominates.
    Clip(u8),
    /// `Math.abs`.
    Abs,
    /// `Math.min` / `Math.max` of two operands (a longer list folds).
    Min,
    Max,
    /// `Math.floor`, `ceil`, `round`, `trunc`: the identity on integers.
    Integral,
    /// `Math.sign`.
    Sign,
    /// `Math.clz32`: 0..=32 whatever the operand.
    Clz32,
    /// `Math.imul`: an int32 whatever the operands.
    Imul,
    /// `Number(x)` (`NumberCoerce`): the operand's Numbers, and its
    /// Strings' values by their codes (`chars_to_number`). Evaluated on the
    /// operand node whole (`Model::to_number_range`).
    ToNumber,
    /// The Strings of the left operand that pass a comparison with a
    /// one-unit String literal (`CLIP_LT` ...): its code interval clipped.
    /// The result's code interval is the rule's (`Node::cruled`).
    SClip(u8),
}

pub const CLIP_LT: u8 = 0;
pub const CLIP_LE: u8 = 1;
pub const CLIP_GT: u8 = 2;
pub const CLIP_GE: u8 = 3;

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
            RangeOp::Clip(CLIP_LT) => "clip-lt",
            RangeOp::Clip(CLIP_LE) => "clip-le",
            RangeOp::Clip(CLIP_GT) => "clip-gt",
            RangeOp::Clip(_) => "clip-ge",
            RangeOp::Abs => "abs",
            RangeOp::Min => "min",
            RangeOp::Max => "max",
            RangeOp::Integral => "integral",
            RangeOp::Sign => "sign",
            RangeOp::Clz32 => "clz32",
            RangeOp::Imul => "imul",
            RangeOp::ToNumber => "tonumber",
            RangeOp::SClip(_) => "sclip",
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
    let unary = matches!(
        op,
        RangeOp::Neg
            | RangeOp::BitNot
            | RangeOp::Step(_)
            | RangeOp::Abs
            | RangeOp::Integral
            | RangeOp::Sign
            | RangeOp::Clz32
    );
    if l.is_empty() || (!unary && r.is_empty()) {
        return Iv::EMPTY;
    }
    match op {
        // At most one unbounded operand: no sum reaches an infinity.
        RangeOp::Add | RangeOp::Sub if l.big() && r.big() => Iv::TOP,
        RangeOp::Add if !l.is_top() && !r.is_top() => Iv::int(l.lo + r.lo, l.hi + r.hi),
        RangeOp::Sub if !l.is_top() && !r.is_top() => Iv::int(l.lo - r.hi, l.hi - r.lo),
        RangeOp::Mul if l.big() || r.big() => Iv::TOP,
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
        // An integer that passes `x < e` is at most `e.hi - 1` (and so on);
        // an unbounded test bound keeps the interval.
        RangeOp::Clip(k) if !l.is_top() => {
            if r.is_top() {
                return l;
            }
            match k {
                CLIP_LT => Iv::int(l.lo, l.hi.min(r.hi - 1)),
                CLIP_LE => Iv::int(l.lo, l.hi.min(r.hi)),
                CLIP_GT => Iv::int(l.lo.max(r.lo + 1), l.hi),
                _ => Iv::int(l.lo.max(r.lo), l.hi),
            }
        }
        RangeOp::Abs if !l.is_top() => {
            if l.lo >= 0 {
                l
            } else if l.hi <= 0 {
                Iv::int(-l.hi, -l.lo)
            } else {
                Iv::int(0, (-l.lo).max(l.hi))
            }
        }
        RangeOp::Min if !l.is_top() && !r.is_top() => Iv::int(l.lo.min(r.lo), l.hi.min(r.hi)),
        RangeOp::Max if !l.is_top() && !r.is_top() => Iv::int(l.lo.max(r.lo), l.hi.max(r.hi)),
        RangeOp::Integral if !l.is_top() => l,
        RangeOp::Sign if !l.is_top() => Iv::int(l.lo.signum(), l.hi.signum()),
        RangeOp::Clz32 => Iv::int(0, 32),
        RangeOp::Imul => Iv::I32,
        RangeOp::Shl => Iv::I32,
        RangeOp::BitNot => {
            let a = to_int32(l);
            Iv::int(-a.hi - 1, -a.lo - 1)
        }
        _ => Iv::TOP,
    }
}

/// N6 (`Model::const_numbers`): the exact result of a bitwise operator where
/// [`eval`] gives only its int32 hull. Two operands that are each one int32
/// value give the one value the operator computes; a left shift by one
/// count scales both bounds when neither leaves the int32 range. `None`:
/// `eval` decides. Each result lies inside `eval`'s for the same operands.
pub fn eval_exact(op: RangeOp, l: Iv, r: Iv) -> Option<Iv> {
    let point = |a: Iv| (!a.is_empty() && !a.is_top() && a.lo == a.hi).then_some(a.lo);
    if l.is_empty() || l.is_top() || !l.within(Iv::I32) {
        return None;
    }
    // ToUint32 of the count, masked to five bits.
    let count = point(r).map(|k| (k & 31) as u32);
    if op == RangeOp::Shl {
        let (lo, hi) = (l.lo << count?, l.hi << count?);
        let iv = Iv { lo, hi };
        return iv.within(Iv::I32).then_some(iv);
    }
    let a = point(l)? as i32;
    let v = match op {
        RangeOp::UShr => i64::from((a as u32) >> count?),
        RangeOp::BitAnd | RangeOp::BitOr | RangeOp::BitXor => {
            let b = point(r).filter(|_| r.within(Iv::I32))? as i32;
            i64::from(match op {
                RangeOp::BitAnd => a & b,
                RangeOp::BitOr => a | b,
                _ => a ^ b,
            })
        }
        _ => return None,
    };
    Some(Iv { lo: v, hi: v })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_bitwise_results_lie_inside_the_hull() {
        let p = |v: i64| Iv::int(v, v);
        assert_eq!(eval_exact(RangeOp::Shl, p(1), p(17)), Some(p(131072)));
        assert_eq!(eval_exact(RangeOp::Shl, p(1), p(49)), Some(p(131072)));
        assert_eq!(eval_exact(RangeOp::Shl, p(-3), p(4)), Some(p(-48)));
        assert_eq!(
            eval_exact(RangeOp::Shl, Iv::int(0, 7), p(4)),
            Some(Iv::int(0, 112))
        );
        // `1 << 31` is int32 MIN: the bounds leave the range.
        assert_eq!(eval_exact(RangeOp::Shl, p(1), p(31)), None);
        assert_eq!(eval_exact(RangeOp::Shl, Iv::int(0, 7), Iv::int(1, 2)), None);
        assert_eq!(eval_exact(RangeOp::Shl, Iv::TOP, p(1)), None);
        assert_eq!(eval_exact(RangeOp::Shl, Iv::int(0, 1 << 40), p(1)), None);
        assert_eq!(
            eval_exact(RangeOp::BitOr, p(1 << 17), p(1 << 18)),
            Some(p(393216))
        );
        assert_eq!(eval_exact(RangeOp::BitAnd, p(-1), p(255)), Some(p(255)));
        assert_eq!(eval_exact(RangeOp::BitXor, p(-1), p(1)), Some(p(-2)));
        assert_eq!(eval_exact(RangeOp::BitOr, p(1), Iv::int(0, 1)), None);
        assert_eq!(eval_exact(RangeOp::BitOr, p(1), p(1 << 40)), None);
        assert_eq!(
            eval_exact(RangeOp::UShr, p(-1), p(0)),
            Some(p(u32::MAX as i64))
        );
        assert_eq!(eval_exact(RangeOp::Add, p(1), p(1)), None);
        for (op, l, r) in [
            (RangeOp::Shl, p(5), p(3)),
            (RangeOp::Shl, Iv::int(-4, 9), p(2)),
            (RangeOp::BitAnd, p(-7), p(12)),
            (RangeOp::BitOr, p(6), p(9)),
            (RangeOp::BitXor, p(6), p(-9)),
            (RangeOp::UShr, p(-6), p(3)),
        ] {
            assert!(eval_exact(op, l, r).unwrap().within(eval(op, l, r)));
        }
    }

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
        // Past 2^53 a sum stays an integer, unbounded.
        assert_eq!(
            eval(RangeOp::Add, Iv::int(0, 1 << 53), Iv::int(1, 1)),
            Iv::int(1, BIG)
        );
        // Two unbounded operands may overflow; a product of one may too.
        assert!(eval(RangeOp::Add, Iv::int(0, BIG), Iv::int(0, BIG)).is_top());
        assert!(eval(RangeOp::Mul, Iv::int(1, BIG), Iv::int(2, 2)).is_top());
        // A test bound clips an unbounded counter back.
        assert_eq!(
            eval(RangeOp::Clip(CLIP_LT), Iv::int(0, BIG), Iv::int(8, 8)),
            Iv::int(0, 7)
        );
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
    fn a_loop_test_clips_the_counter() {
        assert_eq!(
            eval(RangeOp::Clip(CLIP_LT), Iv::int(0, 200), Iv::int(128, 128)),
            Iv::int(0, 127)
        );
        assert_eq!(
            eval(RangeOp::Clip(CLIP_GE), Iv::int(-5, 9), Iv::int(0, 0)),
            Iv::int(0, 9)
        );
        assert!(eval(RangeOp::Clip(CLIP_LT), Iv::TOP, Iv::int(8, 8)).is_top());
        assert_eq!(
            eval(RangeOp::Clip(CLIP_LT), Iv::int(0, 9), Iv::TOP),
            Iv::int(0, 9)
        );
    }

    #[test]
    fn widening_climbs_a_finite_ladder() {
        assert_eq!(widen(Iv::int(0, 3), Iv::int(0, 4)), Iv::int(0, 7));
        assert_eq!(widen(Iv::int(0, 7), Iv::int(-1, 7)), Iv::int(-1, 7));
        assert_eq!(widen(Iv::int(-1, 7), Iv::int(-2, 7)), Iv::int(-2, 7));
        assert_eq!(
            widen(Iv::int(0, 1 << 33), Iv::int(0, (1 << 33) + 1)),
            Iv::int(0, 1 << 53)
        );
        assert!(widen(Iv::int(0, 1 << 53), Iv::TOP).is_top());
    }

    #[test]
    fn math_builtins_keep_integer_bounds() {
        assert_eq!(eval(RangeOp::Abs, Iv::int(-6, 4), Iv::EMPTY), Iv::int(0, 6));
        assert_eq!(
            eval(RangeOp::Abs, Iv::int(-6, -2), Iv::EMPTY),
            Iv::int(2, 6)
        );
        assert_eq!(
            eval(RangeOp::Min, Iv::int(0, 24), Iv::int(24, 24)),
            Iv::int(0, 24)
        );
        assert_eq!(
            eval(RangeOp::Max, Iv::int(-3, 2), Iv::int(0, 0)),
            Iv::int(0, 2)
        );
        assert_eq!(
            eval(RangeOp::Sign, Iv::int(-5, 0), Iv::EMPTY),
            Iv::int(-1, 0)
        );
        assert!(eval(RangeOp::Integral, Iv::TOP, Iv::EMPTY).is_top());
        assert!(eval(RangeOp::Min, Iv::TOP, Iv::int(1, 1)).is_top());
        assert_eq!(eval(RangeOp::Clz32, Iv::TOP, Iv::EMPTY), Iv::int(0, 32));
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
