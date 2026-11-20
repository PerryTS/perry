// Issue #2418 — Number.prototype.toFixed must round genuine half values
// AWAY FROM ZERO (ECMA-262 §21.1.3.3 picks the larger n on a tie), not
// half-to-even like Rust's `format!`. It must also emit exponential form
// for |x| >= 1e21 (the result is ToString(x), per spec step 9).
//
// The tricky part: precision-artifact halves (e.g. 0.015*100 == 1.5 in f64,
// but 0.015 is really 0.01499…) must STILL round on the true value, so those
// stay byte-identical to Node's Grisu formatter. All lines compare
// byte-for-byte against `node --experimental-strip-types`.

// Genuine halves — round away from zero.
console.log((1234.5).toFixed(0)); // 1235
console.log((2.5).toFixed(0)); // 3
console.log((0.5).toFixed(0)); // 1
console.log((1.5).toFixed(0)); // 2
console.log((3.5).toFixed(0)); // 4
console.log((255.5).toFixed(0)); // 256

// Negative genuine halves — magnitude rounds away from zero.
console.log((-1234.5).toFixed(0)); // -1235
console.log((-2.5).toFixed(0)); // -3
console.log((-0.5).toFixed(0)); // -1

// Genuine halves at higher precision.
console.log((0.125).toFixed(2)); // 0.13
console.log((2.45).toFixed(1)); // 2.5
console.log((1.45).toFixed(1)); // 1.5

// Precision-artifact halves — round on the true (sub-half) value.
console.log((0.015).toFixed(2)); // 0.01
console.log((8.575).toFixed(2)); // 8.57
console.log((1.005).toFixed(2)); // 1.00
console.log((1.255).toFixed(2)); // 1.25
console.log((0.135).toFixed(2)); // 0.14
console.log((9.95).toFixed(1)); // 9.9

// >= 1e21 — exponential form, not zero-padded decimals.
console.log((1e21).toFixed(2)); // 1e+21
console.log((1e21).toFixed(0)); // 1e+21
console.log((-1e21).toFixed(2)); // -1e+21
console.log((5e20).toFixed(2)); // 500000000000000000000.00 (still < 1e21)

// Non-half ordinary cases — unaffected.
console.log((123.456).toFixed(2)); // 123.46
console.log((1.999).toFixed(2)); // 2.00
console.log((1.23456789).toFixed(5)); // 1.23457
console.log((100).toFixed(2)); // 100.00
console.log((0).toFixed(2)); // 0.00
