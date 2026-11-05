// Test Math extras and Number formatting methods

// Math.clz32 — count leading zeros in 32-bit integer
console.log(Math.clz32(1)); // 31
console.log(Math.clz32(1000)); // 22
console.log(Math.clz32(0)); // 32

// Math.fround — nearest 32-bit float
console.log(Math.fround(1.337)); // 1.3370000123977661
console.log(Math.fround(5.5)); // 5.5
console.log(Math.fround(0)); // 0

// Math.cbrt — cube root
console.log(Math.cbrt(27)); // 3
console.log(Math.cbrt(64)); // 4
console.log(Math.cbrt(-8)); // -2

// Math.hypot — hypotenuse / Euclidean distance
console.log(Math.hypot(3, 4)); // 5
console.log(Math.hypot(5, 12)); // 13
console.log(Math.hypot()); // 0

// Math.expm1 — exp(x) - 1 (accurate near zero)
console.log(Math.expm1(0)); // 0
console.log(Math.expm1(1).toFixed(10)); // 1.7182818285

// Math.log1p — log(1 + x) (accurate near zero)
console.log(Math.log1p(0)); // 0
console.log(Math.log1p(1).toFixed(10)); // 0.6931471806

// Hyperbolic functions
console.log(Math.sinh(0)); // 0
console.log(Math.cosh(0)); // 1
console.log(Math.tanh(0)); // 0
console.log(Math.tanh(Infinity)); // 1

// Inverse hyperbolic functions
console.log(Math.asinh(0)); // 0
console.log(Math.acosh(1)); // 0
console.log(Math.atanh(0)); // 0

// Math.sign
console.log(Math.sign(-5)); // -1
console.log(Math.sign(0)); // 0
console.log(Math.sign(3)); // 1

// Math.trunc
console.log(Math.trunc(3.7)); // 3
console.log(Math.trunc(-3.7)); // -3
console.log(Math.trunc(0.1)); // 0

// Number.prototype.toPrecision
console.log((1.337).toPrecision(3)); // 1.34
console.log((123.456).toPrecision(5)); // 123.46
console.log((0.00123).toPrecision(2)); // 0.0012

// Number.prototype.toExponential
console.log((12345).toExponential(2)); // 1.23e+4
console.log((0.0045).toExponential(1)); // 4.5e-3
console.log((1).toExponential(0)); // 1e+0

// Number.parseFloat and Number.parseInt (same as globals)
console.log(Number.parseFloat("3.14")); // 3.14
console.log(Number.parseInt("42")); // 42
console.log(Number.parseInt("0xff", 16)); // 255
console.log(Number.parseFloat === parseFloat); // true
console.log(Number.parseInt === parseInt); // true

// Number.isFinite, Number.isNaN, Number.isInteger, Number.isSafeInteger
console.log(Number.isFinite(42)); // true
console.log(Number.isFinite(Infinity)); // false
console.log(Number.isNaN(NaN)); // true
console.log(Number.isNaN(42)); // false
console.log(Number.isInteger(5)); // true
console.log(Number.isInteger(5.5)); // false
console.log(Number.isSafeInteger(Number.MAX_SAFE_INTEGER)); // true
console.log(Number.isSafeInteger(Number.MAX_SAFE_INTEGER + 1)); // false

// Expected output:
// 31
// 22
// 32
// 1.3370000123977661
// 5.5
// 0
// 3
// 4
// -2
// 5
// 13
// 0
// 0
// 1.7182818285
// 0
// 0.6931471806
// 0
// 1
// 0
// 1
// 0
// 0
// 0
// -1
// 0
// 1
// 3
// -3
// 0
// 1.34
// 123.46
// 0.0012
// 1.23e+4
// 4.5e-3
// 1e+0
// 3.14
// 42
// 255
// true
// true
// true
// false
// true
// false
// true
// false
// true
// false
