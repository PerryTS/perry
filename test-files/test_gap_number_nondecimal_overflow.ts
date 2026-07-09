// Number() on a non-decimal literal wider than u64 must round to nearest f64
// (ToNumber, round-to-nearest-even), not become NaN. Small values and error
// cases are unchanged. #6079.
console.log(Number("0xFFFFFFFFFFFFFFFFF")); // 17 hex digits, > u64::MAX
console.log(Number("0xffffffffffffffff")); // exactly 16 hex digits (u64::MAX)
console.log(Number("0b" + "1".repeat(70))); // 70-bit binary
console.log(Number("0o" + "7".repeat(30))); // large octal
console.log(Number("0x10"), Number("0b101"), Number("0o17")); // small: 16 5 15
console.log(Number("0x"), Number("0b12"), Number("0o18")); // NaN NaN NaN
console.log(Number("0X1F"), Number("0O17"), Number("0B11")); // uppercase prefixes
// Correctly-rounded past 2^53 — compare the *double* (===), not its printed
// form (Perry's large-integer number→string differs from V8's shortest
// round-trip; tracked separately). A naive f64 accumulation mis-rounds here.
console.log(Number("0x1fffffffffffff") === 2 ** 53 - 1); // exact
console.log(Number("0x20000000000001") === 2 ** 53); // ties to even (down)
console.log(Number("0x20000000000003") === 2 ** 53 + 4); // rounds up
console.log(Number("0x3fffffffffffff8") === 2 ** 58); // rounds to 2^58
console.log(Number("0x" + "f".repeat(260)) === Infinity); // beyond f64::MAX
