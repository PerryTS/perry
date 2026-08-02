// #7238 — an i64 function specialization must not stand in for `number`
// arithmetic it cannot prove exact.
//
// `emit_i64_specializations` re-emitted a whole `number`-typed function body in
// i64 registers and wrapped it in an f64 shim that `fptosi`d every argument and
// `sitofp`d the result. Two independent halves of the contract went unchecked:
//
//   1. OVERFLOW. i64 `add`/`sub`/`mul` compute the exact two's-complement
//      result. JS evaluates the same chain in doubles, rounding at every
//      operator, so the two agree only while each intermediate satisfies
//      |v| <= 2^53. Past that the answers merely differ; past 2^63 the i64
//      chain wraps and the sign flips.
//   2. ARGUMENT TRUNCATION. A `number` parameter is a double. The wrapper's
//      `fptosi double %arg to i64` truncated a fractional argument on entry, so
//      the whole body ran on the wrong value — and `sitofp` on the way out
//      cannot represent a fractional result at all.
//
// Self-recursion is what makes the specialized body actually reachable
// (straight-line callers get HIR-inlined first), so most shapes below recurse.

// ---- 1. overflow past 2^63: the exact i64 chain wrapped negative ----
function grow(n: number, acc: number): number {
  return n === 0 ? acc : grow(n - 1, acc * 3 + 1);
}
console.log("grow40:", grow(40, 1));

// ---- 2. overflow past 2^53 but below 2^63: wrong, not negative ----
console.log("grow35:", grow(35, 1));
console.log("grow30:", grow(30, 1));

// ---- 3. fractional argument truncated on entry ----
function frac(n: number, acc: number): number {
  return n === 0 ? acc : frac(n - 1, acc * 2);
}
console.log("frac:", frac(3, 0.5));
console.log("fracNeg:", frac(2, -0.25));
console.log("fracZero:", frac(4, 0.1));

// ---- 4. the 2^53 boundary from both sides ----
function dbl(n: number, acc: number): number {
  return n === 0 ? acc : dbl(n - 1, acc * 2);
}
console.log("pow2_52:", dbl(52, 1)); // 2^52 — exact both ways
console.log("pow2_53:", dbl(53, 1)); // 2^53 — the last exact integer
console.log("pow2_54:", dbl(54, 1)); // 2^54 — ulp is 2, still even so exact
console.log("pow2_53_plus:", dbl(53, 3)); // 3 * 2^53 — exact (odd * 2^53)

function addOne(n: number, acc: number): number {
  return n === 0 ? acc : addOne(n - 1, acc + 1);
}
// 2^53 - 2 -> 2^53 - 1 -> 2^53: still exact.
console.log("below53:", addOne(2, 9007199254740990));
// One step past: JS saturates (2^53 + 1 is not representable), i64 does not.
console.log("across53:", addOne(4, 9007199254740990));
console.log("across53neg:", addOne(4, -9007199254740994));

function subOne(n: number, acc: number): number {
  return n === 0 ? acc : subOne(n - 1, acc - 1);
}
console.log("down53:", subOne(4, -9007199254740990));

// ---- 5. chains that must stay exact (and, where specialized, stay fast) ----
function fib(n: number): number {
  if (n <= 1) return n;
  return fib(n - 1) + fib(n - 2);
}
console.log("fib25:", fib(25));
console.log("fib40:", fib(40));

function fact(n: number, acc: number): number {
  return n <= 1 ? acc : fact(n - 1, acc * n);
}
console.log("fact18:", fact(18, 1)); // 6402373705728000 < 2^53
console.log("fact20:", fact(20, 1)); // 2432902008176640000 — past 2^53
console.log("fact25:", fact(25, 1)); // past 2^63

function sumTo(n: number, acc: number): number {
  return n === 0 ? acc : sumTo(n - 1, acc + n);
}
console.log("sumTo1000:", sumTo(1000, 0));

function tri(n: number): number {
  if (n <= 0) return 0;
  return n + tri(n - 1);
}
console.log("tri100:", tri(100));

// ---- 6. a non-recursive numeric function reached indirectly ----
// Straight-line calls are HIR-inlined before the specialization matters; an
// indirect call through a function value is not, so the wrapper's `fptosi`
// is the thing that actually runs.
function mulAdd(x: number, y: number): number {
  return x * y + 1;
}
const fns: ((a: number, b: number) => number)[] = [mulAdd];
console.log("mulAddDirect:", mulAdd(0.5, 3));
console.log("mulAddIndirect:", fns[0](0.5, 3));

function apply2(f: (a: number, b: number) => number, a: number, b: number): number {
  return f(a, b);
}
console.log("mulAddApplied:", apply2(mulAdd, 1.5, 2.5));
console.log("mulAddBig:", apply2(mulAdd, 94906266, 94906266));

// ---- 7. comparisons inside a specialized body must see the real values ----
function pickLarger(a: number, b: number): number {
  return a > b ? a : b;
}
console.log("pick:", apply2(pickLarger, 0.5, 0.25));

function countDown(n: number, acc: number): number {
  return n < 0.5 ? acc : countDown(n - 1, acc + 1);
}
console.log("countDown:", countDown(3.5, 0));
