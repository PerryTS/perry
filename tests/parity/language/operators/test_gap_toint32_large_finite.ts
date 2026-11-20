// ToInt32 of finite-but-huge values must wrap modulo 2^32, not go through
// LLVM's out-of-range `fptosi` poison window. Pre-fix, `is_known_finite`
// proved only NaN/Inf-freedom, so `(1e20) | 0` and nested integer-local
// multiplies took the unguarded `toint32_fast` (bare fptosi+trunc) and
// printed NaN instead of the wrapped int32. (CodeRabbit review on #5466;
// the hole predates the branch.)

// Big literal: ToInt32(1e20) === 1661992960.
console.log((1e20) | 0);
console.log((-1e20) | 0);
console.log((1e300) | 0);

// Nested multiplies of i32-range locals escape 2^63 while staying finite.
// (The through-a-local form `const big = x*x*x; big | 0` still rides the
// issue-#49 integer-locals accumulator tradeoff — the collector's own doc
// accepts overflow there — so only the inline form is pinned here.)
const a: number = 2147483647;
let x: number = a;
for (let i = 0; i < 1; i++) x = a;
console.log((x * x * x) | 0);

// Just around the 2^63 fptosi boundary.
const c: number = 3037000499;
console.log((c * c) | 0);
console.log((c * c * 2) | 0);

// Sanity: the common in-range shapes keep their exact values.
console.log((2147483647) | 0);
console.log((-2147483648) | 0);
console.log((123.9) | 0);
console.log((x + 1) | 0);
console.log((x * 2) | 0);

// Other bitwise operators share the same coercion path.
console.log((1e20) ^ 0);
console.log((1e20) >> 1);
console.log((1e20) >>> 0);
console.log(~(1e20));
