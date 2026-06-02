const ITERATIONS = 400000;

let a = 1.000000119;
let b = 0.999999881;
let sum = 0.25;

for (let i = 0; i < ITERATIONS; i++) {
  sum = a * b + sum;
  a = a + 0.00000000013;
  b = b - 0.00000000007;
}

console.log("fma_contract:" + sum);
