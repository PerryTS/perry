// Benchmark: Numeric array pointer-free layout
// Measures push + indexed mutation/read on arrays that remain numeric-only.
// This is the positive path for the typed-runtime numeric-array metadata slice.

const SIZE = 250000;
const ITERATIONS = 25;

function buildNumericArray(): number[] {
  const arr: number[] = [];
  for (let i = 0; i < SIZE; i++) {
    arr.push(i);
  }
  return arr;
}

function runNumericArray(): number {
  const arr = buildNumericArray();
  let checksum = 0;

  for (let iter = 0; iter < ITERATIONS; iter++) {
    for (let i = 0; i < arr.length; i++) {
      arr[i] = arr[i] + 1;
    }
    checksum = checksum + arr[0] + arr[arr.length - 1];
  }

  return checksum + arr.length;
}

// Warmup
for (let i = 0; i < 3; i++) {
  runNumericArray();
}

const start = Date.now();
const checksum = runNumericArray();
const elapsed = Date.now() - start;

console.log("numeric_array_numeric:" + elapsed);
console.log("checksum:" + checksum);
