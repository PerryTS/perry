//! #7232: the exactness proof an i32-native chain has to carry.
//!
//! An i32-native chain computes the exact two's-complement low 32 bits of the
//! integer result. JS evaluates the same chain in doubles, rounding at every
//! operator. They agree only while each intermediate is exactly representable
//! as a double (`|v| <= 2^53`), which is what [`i32_chain_magnitude_bits`]
//! proves.
//!
//! Sabotage, both directions:
//!
//! * Widen `F64_EXACT_INTEGER_BITS` past 53 (or drop the `f64_exact_bits` cap
//!   from `combine_i32_chain_bits`) and the `*_diverges_*` tests go green while
//!   the compiler goes back to printing 654583775 for the issue's LCG step.
//! * Drop the `BitAnd` / `Shr`-`UShr` tightening and the `*_stays_exact_*`
//!   tests go red — masked and shifted hash mixing would leave the fast path
//!   for no correctness reason.

use std::collections::{HashMap, HashSet};

use perry_hir::{BinaryOp, Expr};

use super::{i32_chain_magnitude_bits, FlatConstInfo, I32ChainEnv};

/// An environment whose only fact is "locals 1..=4 are integer-valued".
struct Tables {
    i32_slots: HashMap<u32, String>,
    flat_const_arrays: HashMap<u32, FlatConstInfo>,
    array_row_aliases: HashMap<u32, (u32, Box<Expr>)>,
    integer_locals: HashSet<u32>,
    empty_fns: HashSet<u32>,
}

impl Tables {
    fn new() -> Self {
        Self {
            i32_slots: HashMap::new(),
            flat_const_arrays: HashMap::new(),
            array_row_aliases: HashMap::new(),
            integer_locals: (1..=4).collect(),
            empty_fns: HashSet::new(),
        }
    }

    fn env(&self) -> I32ChainEnv<'_> {
        I32ChainEnv {
            i32_slots: &self.i32_slots,
            flat_const_arrays: &self.flat_const_arrays,
            array_row_aliases: &self.array_row_aliases,
            integer_locals: &self.integer_locals,
            clamp3_fns: &self.empty_fns,
            clamp_u8_fns: &self.empty_fns,
            integer_returning_fns: &self.empty_fns,
            i32_identity_fns: &self.empty_fns,
        }
    }
}

fn bits(e: &Expr) -> Option<u32> {
    let tables = Tables::new();
    i32_chain_magnitude_bits(e, tables.env())
}

fn bin(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn mul(left: Expr, right: Expr) -> Expr {
    bin(BinaryOp::Mul, left, right)
}

fn add(left: Expr, right: Expr) -> Expr {
    bin(BinaryOp::Add, left, right)
}

fn and(left: Expr, right: Expr) -> Expr {
    bin(BinaryOp::BitAnd, left, right)
}

/// `x`, an integer-valued local: i32-shaped, magnitude unknown beyond 2^32.
fn x() -> Expr {
    Expr::LocalGet(1)
}

fn y() -> Expr {
    Expr::LocalGet(2)
}

fn byte() -> Expr {
    Expr::Uint8ArrayGet {
        array: Box::new(Expr::LocalGet(3)),
        index: Box::new(Expr::Integer(0)),
    }
}

// ---------------------------------------------------------------------------
// The bug: a product past 2^53 must leave the exact-integer path.
// ---------------------------------------------------------------------------

/// The issue's own expression: `(x * 1103515245 + 12345) & 0x7fffffff`.
/// 32 + 31 = 63 bits of product — past 2^53, so JS has already rounded and an
/// exact `mul i32` would read low bits the double discarded.
#[test]
fn lcg_step_diverges_and_is_rejected() {
    let step = and(
        add(mul(x(), Expr::Integer(1103515245)), Expr::Integer(12345)),
        Expr::Integer(0x7fffffff),
    );
    assert_eq!(bits(&step), None);
    // Every sub-chain that contains the product is rejected with it, so no
    // consumer can pick up a half-exact intermediate.
    assert_eq!(bits(&mul(x(), Expr::Integer(1103515245))), None);
    assert_eq!(
        bits(&add(
            mul(x(), Expr::Integer(1103515245)),
            Expr::Integer(12345)
        )),
        None
    );
}

/// Two i32-range locals multiplied: 64 bits, rejected. This is the general
/// `i * size` / `i * i` shape, which is only exact because the *runtime*
/// values are small — nothing here proves that.
#[test]
fn unbounded_local_square_diverges_and_is_rejected() {
    assert_eq!(bits(&mul(x(), y())), None);
}

/// The cap applies to `Add`/`Sub` too, not just `Mul`: two products that are
/// each exactly at the ceiling sum to 2^54.
#[test]
fn sum_of_two_ceiling_products_is_rejected() {
    // (x & 0x3fffff) * (y & 0x7fffffff) == 22 + 31 == 53 bits: admitted.
    let p = mul(
        and(x(), Expr::Integer(0x3fffff)),
        and(y(), Expr::Integer(0x7fffffff)),
    );
    assert_eq!(bits(&p), Some(53));
    // ...but their sum could reach 2^54.
    assert_eq!(bits(&add(p.clone(), p)), None);
}

/// The exactness boundary itself, from both sides.
#[test]
fn boundary_at_2_pow_53() {
    let exact = mul(
        and(x(), Expr::Integer(0x3fffff)),   // 22 bits
        and(y(), Expr::Integer(0x7fffffff)), // 31 bits
    );
    assert_eq!(bits(&exact), Some(53));
    let one_bit_wider = mul(
        and(x(), Expr::Integer(0x7fffff)),   // 23 bits
        and(y(), Expr::Integer(0x7fffffff)), // 31 bits
    );
    assert_eq!(bits(&one_bit_wider), None);
}

// ---------------------------------------------------------------------------
// The other direction: chains that ARE f64-exact must stay on the fast path.
// ---------------------------------------------------------------------------

/// Java-style string hashing, `h * 31 + c`: 32 + 5 = 37 bits, comfortably
/// exact. Rejecting this would deoptimize every small-multiplier accumulator.
#[test]
fn small_multiplier_stays_exact() {
    assert_eq!(bits(&mul(x(), Expr::Integer(31))), Some(37));
    assert_eq!(bits(&add(mul(x(), Expr::Integer(31)), byte())), Some(38));
}

/// `Math.imul` is *defined* as an exact low-32 multiply, so it is exempt from
/// the rounding rule and its result is an i32 — even with a `>i32::MAX`
/// literal operand and even when the true product is astronomically large.
#[test]
fn math_imul_is_exempt_from_the_rounding_rule() {
    let imul = Expr::MathImul(Box::new(x()), Box::new(Expr::Integer(1103515245)));
    assert_eq!(bits(&imul), Some(32));
    let mixer = Expr::MathImul(Box::new(x()), Box::new(Expr::Integer(0x9e3779b1)));
    assert_eq!(bits(&mixer), Some(32));
    // ...and an imul result feeds an ordinary chain as a 32-bit leaf.
    assert_eq!(bits(&add(imul, Expr::Integer(1))), Some(33));
}

/// Masking with a non-negative literal bounds the operand by the mask, which
/// is what keeps 16x16 mixing on the exact path.
#[test]
fn masked_operands_stay_exact() {
    assert_eq!(bits(&and(x(), Expr::Integer(0xffff))), Some(16));
    assert_eq!(
        bits(&mul(
            and(x(), Expr::Integer(0xffff)),
            and(y(), Expr::Integer(0xffff))
        )),
        Some(32)
    );
}

/// An unsigned/signed shift by a literal count drops that many bits, which is
/// what keeps `(h >>> 16) * K` mixing on the exact path.
#[test]
fn shifted_operands_stay_exact() {
    let hi = bin(BinaryOp::UShr, x(), Expr::Integer(16));
    assert_eq!(bits(&hi), Some(16));
    assert_eq!(bits(&mul(hi, Expr::Integer(0x5bd1e995))), Some(47));
    let arith = bin(BinaryOp::Shr, y(), Expr::Integer(24));
    assert_eq!(bits(&arith), Some(8));
    // A non-literal count keeps the untightened 32.
    assert_eq!(bits(&bin(BinaryOp::UShr, x(), y())), Some(32));
}

/// Byte loads are 8-bit leaves, so byte products stay far inside the ceiling.
#[test]
fn byte_leaves_are_eight_bits() {
    assert_eq!(bits(&byte()), Some(8));
    assert_eq!(bits(&mul(byte(), byte())), Some(16));
}

/// Bitwise results are ToInt32-wrapped, so they reset the bound to 32 and a
/// long masked chain never escalates.
#[test]
fn bitwise_resets_the_bound() {
    assert_eq!(bits(&bin(BinaryOp::BitXor, x(), y())), Some(32));
    assert_eq!(bits(&bin(BinaryOp::BitOr, x(), y())), Some(32));
    assert_eq!(bits(&bin(BinaryOp::Shl, x(), Expr::Integer(3))), Some(32));
    // `v | 0` keeps a tighter incoming bound instead of widening to 32.
    assert_eq!(
        bits(&bin(BinaryOp::BitOr, byte(), Expr::Integer(0))),
        Some(8)
    );
}

/// Leaves the chain never admitted: a `>i32::MAX` literal outside `Math.imul`
/// (its low-32 truncation is not what JS `*` computes) and a local with no
/// integer proof.
#[test]
fn unproven_leaves_are_still_rejected() {
    assert_eq!(bits(&Expr::Integer(3000000000)), None);
    assert_eq!(bits(&Expr::LocalGet(99)), None);
    assert_eq!(bits(&mul(x(), Expr::Integer(3000000000))), None);
}

/// Literal magnitudes are measured, not assumed: the bound of a literal leaf
/// is its own bit width, which is what makes `x * 31` land at 37 rather than
/// the leaf default of 64.
#[test]
fn literal_leaves_carry_their_own_width() {
    assert_eq!(bits(&Expr::Integer(0)), Some(0));
    assert_eq!(bits(&Expr::Integer(1)), Some(1));
    assert_eq!(bits(&Expr::Integer(31)), Some(5));
    assert_eq!(bits(&Expr::Integer(-31)), Some(5));
    assert_eq!(bits(&Expr::Integer(i32::MAX as i64)), Some(31));
    assert_eq!(bits(&Expr::Integer(i32::MIN as i64)), Some(32));
}
