// Repsel Phase 1, #7110: canonical unboxed i32 storage for a monotone loop
// induction variable — a counter that is NOT used as an array index and is NOT
// in `strictly_i32_bounded_locals` (`i++` disqualifies a local there, #6072).
//
// Byte-compared against `node --experimental-strip-types`. Every number below
// is chosen so that a WRONG answer is loud rather than plausible: the values
// sit on the i32 boundary, where an unsound admission wraps to a large negative
// instead of drifting by one.
//
// The three functions the analysis must REFUSE (`overshoot`, `runtimeBound`,
// `accumulate`) are the point of this file as much as the ones it admits. An
// i32 slot for any of them prints a wrapped negative here, and Node prints the
// true value. Keep them.

// --- Admitted: interval endpoints are compile-time i32 constants ------------

const ROUNDS = 4096;

// `i < 2147483647` with a `++` step tops out at exactly INT32_MAX.
function topOut(): number {
  let i = 2147483640;
  for (; i < 2147483647; i++) {}
  return i;
}

// Descending across zero: interval [-2147483641, 10].
function descend(): number {
  let i = 10;
  for (; i > -2147483648; i--) {
    if (i < -2147483640) {
      break;
    }
  }
  return i;
}

// A module-level `const` bound, the `for (let i = 0; i < ITERATIONS; i++)`
// shape. Sums into a float so the accumulator is not itself a candidate.
function moduleConstBound(): number {
  let acc = 0.5;
  for (let i = 0; i < ROUNDS; i++) {
    acc = acc + 0.25;
  }
  return acc;
}

// A `while` with a CONJUNCTIVE guard and a `LocalSet` Add step rather than
// `++` — the 15_mandelbrot `iter` shape.
function whileConjunct(seed: number): number {
  let iter = 0;
  let x = seed;
  while (x < 1000.0 && iter < 100) {
    x = x * 1.5;
    iter = iter + 1;
  }
  return iter;
}

// Two steps per iteration (update + body) and a step larger than one: the
// counter overshoots the bound by the per-iteration total, which the interval
// has to account for.
function twoSteps(): string {
  let a = 0;
  for (; a < 10; a++) {
    a = a + 1;
  }
  let b = 0;
  for (; b < 10; b = b + 4) {}
  return a + "/" + b;
}

// The counter observed through boxed use sites: value-position `++`/`--`,
// string concat, `typeof`, division, negation, JSON.
function observed(): string {
  let out = "";
  let i = 0;
  for (; i < 4; i++) {
    out = out + i++ + ":" + typeof i + ":" + i / 2 + ":" + JSON.stringify({ k: -i }) + ";";
  }
  let j = 3;
  for (; j > 0; j--) {
    out = out + --j + "|";
  }
  return out + "end=" + i + "," + j;
}

// --- Refused: no compile-time i32 interval exists ---------------------------

// `i <= 2147483647` lets the counter reach 2147483648, one past INT32_MAX.
// `break` keeps the test fast; the proof obligation is a property of the loop
// text, not of the trip count.
function overshoot(): number {
  let i = 2147483640;
  for (; i <= 2147483647; i++) {
    if (i > 2147483642) {
      break;
    }
  }
  return i;
}

// A runtime bound is not a constant: #6072's runaway shape, where `limit`
// exceeds INT32_MAX and a wrapped i32 counter spins forever.
function runtimeBound(limit: number): number {
  let i = 2147483640;
  for (; i < limit; i++) {
    if (i > 2147483645) {
      break;
    }
  }
  return i;
}

// A bare accumulator has no guard bounding it. `benchmarks/suite/13_factorial.ts`
// is the same shape at 1e8 iterations, where the true total is 49,950,000,000.
function accumulate(): number {
  let sum = 0;
  for (let i = 0; i < ROUNDS; i++) {
    sum = sum + 1000000;
  }
  return sum;
}

// The two RUNTIME-OBSERVABLE overflow probes. Every other refusal above is only
// checkable in `--opt-report`; these two print a different number if the
// analysis admits them, in three iterations rather than 2^31.
//
// `i` steps by 2e9 under a `<= INT32_MAX` guard, so it exits at 4e9 — a value an
// i32 slot cannot hold. Dropping the interval's `fits_i32` check admits it, the
// step wraps to -294967296, the guard is true again, and the loop runs until the
// `steps` fuse trips: a different printed value, not a hang.
function bigStepOverflow(): number {
  let i = 0;
  let steps = 0;
  for (; i <= 2147483647; i = i + 2000000000) {
    steps = steps + 1;
    if (steps > 5) {
      break;
    }
  }
  return i;
}

// The descending mirror: exits at -4e9.
function bigStepUnderflow(): number {
  let i = 0;
  let steps = 0;
  for (; i >= -2147483648; i = i - 2000000000) {
    steps = steps + 1;
    if (steps > 5) {
      break;
    }
  }
  return i;
}

console.log("topOut:" + topOut());
console.log("descend:" + descend());
console.log("moduleConstBound:" + moduleConstBound());
console.log("whileConjunct:" + whileConjunct(1.0));
console.log("twoSteps:" + twoSteps());
console.log("observed:" + observed());
console.log("overshoot:" + overshoot());
console.log("runtimeBound:" + runtimeBound(2147483653));
console.log("accumulate:" + accumulate());
console.log("bigStepOverflow:" + bigStepOverflow());
console.log("bigStepUnderflow:" + bigStepUnderflow());
