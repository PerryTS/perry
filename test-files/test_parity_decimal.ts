// Behavioral parity test for decimal.js via perry-stdlib.
//
// All inputs are constants. Decimal arithmetic is deterministic and the
// point of these assertions is to demonstrate it avoids the IEEE-754
// rounding errors that plain `number` would produce.

import Decimal from "decimal.js";

const a = new Decimal("0.1");
const b = new Decimal("0.2");
const c = new Decimal("3");

// ── Construction / display ──
console.log("from_number:", new Decimal(42).toString());
console.log("from_string:", new Decimal("3.14159").toString());
console.log("to_number:", a.plus(b).toNumber());
console.log("to_fixed:", a.plus(b).toFixed(4));

// ── Arithmetic — value-form (decimal arg) ──
console.log("plus:", a.plus(b).toString());
console.log("minus:", b.minus(a).toString());
console.log("times:", a.times(b).toString());
console.log("div:", c.div(a).toString());
console.log("mod:", new Decimal("10").mod(c).toString());
console.log("pow:", a.pow(3).toString());

// ── Arithmetic — number-form (plain number arg) ──
console.log("plus_num:", a.plus(0.2).toString());
console.log("minus_num:", b.minus(0.05).toString());
console.log("times_num:", a.times(10).toString());
console.log("div_num:", c.div(2).toString());

// ── Unary ──
console.log("neg:", a.neg().toString());
console.log("abs:", new Decimal("-7.5").abs().toString());
console.log("ceil:", new Decimal("1.2").ceil().toString());
console.log("floor:", new Decimal("1.8").floor().toString());
console.log("round:", new Decimal("1.5").round().toString());
console.log("sqrt:", new Decimal("9").sqrt().toString());

// ── Comparisons — value-form ──
console.log("cmp_lt:", a.cmp(b));
console.log("cmp_eq:", a.cmp(new Decimal("0.1")));
console.log("eq:", a.eq(new Decimal("0.1")));
console.log("gt:", b.gt(a));
console.log("gte:", b.gte(b));
console.log("lt:", a.lt(b));
console.log("lte:", a.lte(a));

// ── Predicates ──
console.log("isZero:", new Decimal("0").isZero());
console.log("isPositive:", a.isPositive());
console.log("isNegative:", new Decimal("-1").isNegative());

/*
@covers
crates/perry-stdlib/src/decimal.rs:
  - js_decimal_abs
  - js_decimal_ceil
  - js_decimal_cmp
  - js_decimal_cmp_value
  - js_decimal_coerce_to_handle
  - js_decimal_div
  - js_decimal_div_number
  - js_decimal_div_value
  - js_decimal_eq
  - js_decimal_eq_value
  - js_decimal_floor
  - js_decimal_from_number
  - js_decimal_from_string
  - js_decimal_gt
  - js_decimal_gt_value
  - js_decimal_gte
  - js_decimal_gte_value
  - js_decimal_is_negative
  - js_decimal_is_positive
  - js_decimal_is_zero
  - js_decimal_lt
  - js_decimal_lt_value
  - js_decimal_lte
  - js_decimal_lte_value
  - js_decimal_minus
  - js_decimal_minus_number
  - js_decimal_minus_value
  - js_decimal_mod
  - js_decimal_mod_value
  - js_decimal_neg
  - js_decimal_plus
  - js_decimal_plus_number
  - js_decimal_plus_value
  - js_decimal_pow
  - js_decimal_round
  - js_decimal_sqrt
  - js_decimal_times
  - js_decimal_times_number
  - js_decimal_times_value
  - js_decimal_to_fixed
  - js_decimal_to_number
  - js_decimal_to_string
*/
