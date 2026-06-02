// Benchmark: Numeric array downgrade path
// Measures arrays that start numeric, receive pointer/string slots, then keep
// mutating numeric regions without losing JS semantics for the downgraded slots.

const SIZE = 250000;
const ITERATIONS = 25;
const OBJECT_INDEX = 125000;
const STRING_INDEX = 125001;

function buildDowngradedArray(): any[] {
  const arr: any[] = [];
  for (let i = 0; i < SIZE; i++) {
    arr.push(i);
  }

  arr[OBJECT_INDEX] = { value: 7 };
  arr[STRING_INDEX] = "numeric-array-downgrade-payload";
  return arr;
}

function runDowngradedArray(): number {
  const arr = buildDowngradedArray();
  let checksum = 0;

  for (let iter = 0; iter < ITERATIONS; iter++) {
    for (let i = 0; i < OBJECT_INDEX; i++) {
      arr[i] = arr[i] + 1;
    }
    for (let i = STRING_INDEX + 1; i < arr.length; i++) {
      arr[i] = arr[i] + 1;
    }

    checksum = checksum + arr[0] + arr[arr.length - 1] + arr[OBJECT_INDEX].value;
    if (arr[STRING_INDEX] !== "") {
      checksum = checksum + 3;
    }
  }

  return checksum + arr.length;
}

// Warmup
for (let i = 0; i < 3; i++) {
  runDowngradedArray();
}

const start = Date.now();
const checksum = runDowngradedArray();
const elapsed = Date.now() - start;

console.log("numeric_array_downgrade:" + elapsed);
console.log("checksum:" + checksum);
