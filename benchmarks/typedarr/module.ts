const L = new Int32Array(4096);
for (let i = 0; i < L.length; i++) L[i] = i - 2048;
let sum = 0;
for (let pass = 0; pass < 1000; pass++) {
  for (let i = 0; i < L.length; i++) sum += L[i] < 0 ? -1 : 1;
}
console.log(sum, L[12]);
