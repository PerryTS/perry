// Benchmark: Recursive Fibonacci
// Tests: function call overhead, recursion, stack management

function fibonacci(n: number): number {
  if (n <= 1) {
    return n;
  }
  return fibonacci(n - 1) + fibonacci(n - 2);
}

const WARMUP_ITERATIONS = 5;
const TIMED_ITERATIONS = 100;
const FIB_N = 35;

// Warmup phase (for JIT fairness)
// #7395: ACCUMULATE the result. With it discarded, Perry proved the call
// pure and dead-code-eliminated the entire loop, so this file reported
// TOTAL:0 and "measured" nothing -- a benchmark that cannot fail, the same
// hazard as a gate whose subject never runs. Node happened not to do the
// same elimination, which is why the two disagreed by ~240x.
let sink = 0;
for (let i = 0; i < WARMUP_ITERATIONS; i++) {
  sink = sink + fibonacci(FIB_N - (i % 2));
}

// Timed phase
const start = Date.now();
for (let i = 0; i < TIMED_ITERATIONS; i++) {
  // #7395: the argument must VARY. With a loop-invariant `fibonacci(FIB_N)`
  // the call hoists out of the loop and runs once -- the checksum stays
  // correct while TOTAL drops to 0, which is how this benchmark reported a
  // ~240x win over Node while doing 1/100th of the work.
  sink = sink + fibonacci(FIB_N - (i % 2));
}
const end = Date.now();

const total = end - start;
const avg = total / TIMED_ITERATIONS;

// Consume `sink` so no optimiser can argue the loops are dead, and assert
// the subject was live: a benchmark reporting zero elapsed time has not
// proved speed, it has proved it did nothing.
if (sink === 0) {
  console.log("ERROR: sink is 0 -- the benchmark body was eliminated");
}
if (total <= 0) {
  console.log("ERROR: TOTAL is " + total + " -- the timed loop did no work");
}
console.log("CHECKSUM:" + sink);
console.log("BENCHMARK:fibonacci");
console.log("TOTAL:" + total);
console.log("ITERATIONS:" + TIMED_ITERATIONS);
console.log("AVG:" + avg);
