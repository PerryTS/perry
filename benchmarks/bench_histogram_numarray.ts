// Benchmark: value-indexed plain-array histogram (#6904, repsel Phase 4a)
// Tests: `counts[v] = (counts[v] || 0) + 1` — data-dependent (non-loop-
// counter) index, hole-defaulting read, numeric read-modify-write on a
// plain `number[]`. This is the shape where typed plain arrays were 26x
// slower than Node before Phase 4a (guarded out-of-line read + js_is_truthy
// + dynamic add + guarded out-of-line write per iteration).
//
// Deterministic: Park-Miller LCG (every intermediate < 2^53, so the value
// sequence is engine-exact) and a printed checksum.

const BUCKETS = 4096; // power of two, mask-provable index
const N = 1_000_000;

function fillData(): number[] {
  const data: number[] = [];
  let seed = 20260728;
  for (let i = 0; i < N; i++) {
    seed = (seed * 48271) % 2147483647;
    data.push(seed);
  }
  return data;
}

function histogram(data: number[]): number[] {
  const counts: number[] = new Array(BUCKETS);
  const mask = BUCKETS - 1;
  for (let i = 0; i < data.length; i++) {
    const v = data[i] & mask;
    counts[v] = (counts[v] || 0) + 1;
  }
  return counts;
}

function checksum(counts: number[]): number {
  let acc = 0;
  for (let i = 0; i < counts.length; i++) {
    acc = (acc + (counts[i] || 0) * (i + 1)) % 1000000007;
  }
  return acc;
}

const data = fillData();

const WARMUP_ITERATIONS = 3;
const TIMED_ITERATIONS = 20;

let check = 0;
for (let i = 0; i < WARMUP_ITERATIONS; i++) {
  check = checksum(histogram(data));
}

const start = Date.now();
for (let i = 0; i < TIMED_ITERATIONS; i++) {
  check = checksum(histogram(data));
}
const end = Date.now();

const total = end - start;
const avg = total / TIMED_ITERATIONS;

console.log("BENCHMARK:histogram_numarray");
console.log("CHECKSUM:" + check);
console.log("TOTAL:" + total);
console.log("ITERATIONS:" + TIMED_ITERATIONS);
console.log("AVG:" + avg);
