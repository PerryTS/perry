// Edge-case tests for numeric types: integer/float precision, large numbers,
// special values (NaN, Infinity, -0), number methods, numeric coercion
// Targets bugs like: negative number equality, INT32 vs f64 mismatch,
// floating point comparison

// --- Integer arithmetic ---
console.log(1 + 1);           // 2
console.log(1000000 * 1000);  // 1000000000
console.log(7 % 3);           // 1
console.log((-7) % 3);        // -1

// --- Float arithmetic ---
console.log(1.5 + 2.5);     // 4
console.log(3.14 * 2);      // 6.28
console.log(10.0 / 3.0);    // 3.3333333333333335

// --- Negative numbers ---
console.log(-1 + -1);       // -2
console.log(-5 * -3);       // 15
console.log(-10 / 2);       // -5
console.log(-1 - -1);       // 0

// --- Negative number comparisons (NaN-boxing regression) ---
console.log(-1 < 0);         // true
console.log(-1 > -2);        // true
console.log(-1 === -1);      // true
console.log(-1 !== 0);       // true
console.log(-0.5 < 0);       // true
console.log(-0.5 > -1);      // true

// --- Large integers ---
console.log(Number.MAX_SAFE_INTEGER);   // 9007199254740991
console.log(Number.MIN_SAFE_INTEGER);   // -9007199254740991
console.log(Number.MAX_SAFE_INTEGER + 1 === Number.MAX_SAFE_INTEGER + 2);  // true (precision loss)

// --- Special values ---
console.log(NaN);            // NaN
console.log(Infinity);       // Infinity
console.log(-Infinity);      // -Infinity

// --- NaN behavior ---
console.log(NaN === NaN);    // false
console.log(NaN !== NaN);    // true
console.log(isNaN(NaN));     // true
console.log(isNaN(42));      // false
console.log(isNaN(0 / 0));   // true

// --- Infinity behavior ---
console.log(Infinity > 1000000);  // true
console.log(-Infinity < -1000000); // true
console.log(Infinity + 1);        // Infinity
console.log(Infinity - Infinity);  // NaN
console.log(Infinity * 0);        // NaN
console.log(1 / Infinity);        // 0
console.log(isFinite(42));         // true
console.log(isFinite(Infinity));   // false
console.log(isFinite(NaN));        // false

// --- Division edge cases ---
console.log(1 / 0);     // Infinity
console.log(-1 / 0);    // -Infinity
console.log(0 / 0);     // NaN

// --- Number methods ---
console.log((42).toString());      // 42
console.log((255).toString(16));   // ff
console.log((7).toString(2));      // 111
console.log((42.567).toFixed(2));  // 42.57
console.log((42.567).toFixed(0));  // 43

// --- parseInt / parseFloat ---
console.log(parseInt("42"));       // 42
console.log(parseInt("42abc"));    // 42
console.log(parseInt("abc"));      // NaN
console.log(parseInt("0xFF", 16)); // 255
console.log(parseInt("111", 2));   // 7
console.log(parseFloat("3.14"));   // 3.14
console.log(parseFloat("3.14abc")); // 3.14

// --- Number() conversion ---
console.log(Number("42"));       // 42
console.log(Number("3.14"));     // 3.14
console.log(Number(""));         // 0
console.log(Number(true));       // 1
console.log(Number(false));      // 0
console.log(Number(null));       // 0

// --- Math functions edge cases ---
console.log(Math.floor(3.9));     // 3
console.log(Math.floor(-3.1));    // -4
console.log(Math.ceil(3.1));      // 4
console.log(Math.ceil(-3.9));     // -3
console.log(Math.round(0.5));     // 1
console.log(Math.round(-0.5));    // 0
console.log(Math.trunc(3.9));     // 3
console.log(Math.trunc(-3.9));    // -3

console.log(Math.abs(-42));       // 42
console.log(Math.abs(42));        // 42
console.log(Math.abs(-0));        // 0

console.log(Math.max(1, 2, 3));   // 3
console.log(Math.min(1, 2, 3));   // 1
console.log(Math.max(-1, -2, -3)); // -1
console.log(Math.min(-1, -2, -3)); // -3

console.log(Math.pow(2, 0));      // 1
console.log(Math.pow(2, -1));     // 0.5
console.log(Math.pow(-2, 3));     // -8

console.log(Math.sqrt(0));        // 0
console.log(Math.sqrt(1));        // 1
console.log(Math.sqrt(4));        // 2

console.log(Math.log(1));         // 0
console.log(Math.log2(8));        // 3
console.log(Math.log10(1000));    // 3

// --- Exponentiation ---
console.log(2 ** 0);     // 1
console.log(2 ** 10);    // 1024
console.log(2 ** -1);    // 0.5
console.log((-2) ** 3);  // -8

// --- Modulo with negative numbers ---
console.log(5 % 3);       // 2
console.log(-5 % 3);      // -2
console.log(5 % -3);      // 2
console.log(-5 % -3);     // -2

// --- Number comparison with explicit types ---
function numEquals(a: number, b: number): boolean {
    return a === b;
}

console.log(numEquals(5, 5));     // true
console.log(numEquals(5, 6));     // false
console.log(numEquals(-1, -1));   // true
console.log(numEquals(0.1 + 0.2, 0.3));  // false (FP precision)

// --- Numeric literals ---
console.log(0xFF);     // 255
console.log(0b1010);   // 10
console.log(0o77);     // 63
console.log(1e3);      // 1000
console.log(1.5e2);    // 150
console.log(1e-3);     // 0.001

// --- Number.isInteger ---
console.log(Number.isInteger(42));     // true
console.log(Number.isInteger(42.0));   // true
console.log(Number.isInteger(42.5));   // false
console.log(Number.isInteger(NaN));    // false
console.log(Number.isInteger(Infinity)); // false

// --- Number.isFinite ---
console.log(Number.isFinite(42));        // true
console.log(Number.isFinite(Infinity));  // false
console.log(Number.isFinite(NaN));       // false

// --- Number.isNaN ---
console.log(Number.isNaN(NaN));     // true
console.log(Number.isNaN(42));      // false
console.log(Number.isNaN("NaN"));   // false (unlike global isNaN)

// --- Arithmetic overflow behavior ---
const big = 1e308;
console.log(big * 2);     // Infinity
console.log(-big * 2);    // -Infinity

// --- Smallest representable values ---
console.log(Number.EPSILON > 0);          // true
console.log(1 + Number.EPSILON > 1);      // true

// --- Integer loop counter used in float context ---
let floatSum = 0;
for (let i = 0; i < 10; i++) {
    floatSum = floatSum + i * 0.1;
}
console.log(Math.round(floatSum * 100) / 100);  // 4.5
