// Benchmark: Integer Arithmetic Operations
// Tests: tight loops, register operations

function runArithmeticBenchmark(): number {
  const ITERATIONS = 10000000;
  let result = 0;
  let a = 12345678;
  let b = 87654321;

  for (let i = 0; i < ITERATIONS; i++) {
    // Mix of arithmetic operations
    result = result + (a % 1000);
    result = result - (b % 1000);
    result = result + ((a * 3) % 10000);
    result = result - ((b * 2) % 10000);

    // Update a and b
    a = a + 1;
    b = b - 1;

    // Keep values bounded
    if (a > 100000000) {
      a = 12345678;
    }
    if (b < 0) {
      b = 87654321;
    }
  }

  return result;
}

const WARMUP_ITERATIONS = 5;
const TIMED_ITERATIONS = 100;

// Warmup phase (for JIT fairness)
// #7395: ACCUMULATE the result. With it discarded, Perry proved the call
// pure and dead-code-eliminated the entire loop, so this file reported
// TOTAL:0 and "measured" nothing -- a benchmark that cannot fail, the same
// hazard as a gate whose subject never runs. Node happened not to do the
// same elimination, which is why the two disagreed by ~240x.
let sink = 0;
for (let i = 0; i < WARMUP_ITERATIONS; i++) {
  sink = sink + runArithmeticBenchmark();
}

// Timed phase
const start = Date.now();
for (let i = 0; i < TIMED_ITERATIONS; i++) {
  sink = sink + runArithmeticBenchmark();
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
console.log("BENCHMARK:arithmetic");
console.log("TOTAL:" + total);
console.log("ITERATIONS:" + TIMED_ITERATIONS);
console.log("AVG:" + avg);
