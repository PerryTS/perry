// Benchmark: String concatenation
// Measures string allocation and concatenation
const ITERATIONS = 1000000;

let result = "";
const start = Date.now();
for (let i = 0; i < ITERATIONS; i++) {
    result = result + "x";
}
const elapsed = Date.now() - start;

// Observe materialized bytes, not only the length metadata that the optimizer
// can derive from ITERATIONS. Keep the scan outside the timed concatenation
// region and stride it so verification stays cheap.
let checksum = 0;
for (let i = 0; i < result.length; i += 997) {
    checksum = checksum + result.charCodeAt(i);
}

console.log("string_concat:" + elapsed);
console.log("length:" + result.length);
console.log("checksum:" + checksum);
