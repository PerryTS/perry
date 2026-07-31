// Liveness fixture for the monotone loop-induction i32 range proof (#7110).
//
// `fixture_canonical_slots.ts` proves canonical-i32 on STRAIGHT-LINE bitwise
// locals and says so in its own comment — it deliberately avoids loops, because
// before #7110 a loop counter could not select the canonical rep at all. This
// fixture is the complement: every canonical-i32 promotion in it comes from the
// loop-induction rule and from nothing else. There is no bitwise mixing, no
// `| 0`, no `>>> 0`, and no array indexing anywhere, so if
// `collect_loop_bounded_i32_locals` returns the empty set this file's
// canonical-i32 count is zero and the census goes red.
//
// The two locals it must NOT promote are here on purpose: an unadmitted
// counter and an unbounded accumulator keep the fixture from being satisfied by
// any rule that simply says yes to proven-integer locals.
//
// Requirements shared with the other canonical-slot fixtures: plain synchronous
// function bodies (async/generator bodies are context-excluded), and no closure
// capture of the candidate locals.

const ROUNDS = 4096;

// PROMOTES. A bare `for` counter with a module-level `const` bound: not
// index-used, and `i++` keeps it out of `strictly_i32_bounded_locals`.
// Interval [0, 4095].
function countUp(): number {
  let last = 0.5;
  for (let i = 0; i < ROUNDS; i++) {
    last = last + 0.25;
  }
  return last;
}

// PROMOTES. A `while` whose guard is a CONJUNCTION and whose step is a
// `LocalSet` Add rather than `++` — the 15_mandelbrot `iter` shape.
// Interval [0, 100].
function iterate(seed: number): number {
  let iter = 0;
  let x = seed;
  while (x < 1000.0 && iter < 100) {
    x = x * 1.5;
    iter = iter + 1;
  }
  return iter;
}

// DOES NOT PROMOTE. `i <= 2147483647` lets the counter reach 2147483648, one
// past INT32_MAX. Node prints 2147483648 here; an i32 slot would print
// -2147483648. `break` keeps the fixture fast without weakening the proof
// obligation, which is a property of the loop text, not of the trip count.
function overshoot(): number {
  let i = 2147483640;
  for (; i <= 2147483647; i++) {
    if (i > 2147483642) {
      break;
    }
  }
  return i;
}

// DOES NOT PROMOTE. A bare accumulator: `sum` has no guard bounding it, and
// 13_factorial's version of this really does reach 4.995e10.
function accumulate(): number {
  let sum = 0;
  for (let i = 0; i < ROUNDS; i++) {
    sum = sum + 1000000;
  }
  return sum;
}

console.log(
  "loopBounded:" +
    countUp() +
    ":" +
    iterate(1.0) +
    ":" +
    overshoot() +
    ":" +
    accumulate(),
);
