class Arena {
  left: Int32Array;
  constructor(n: number) { this.left = new Int32Array(n); }
}
const a = new Arena(4096);
for (let i = 0; i < 4096; i++) a.left[i] = i - 2048;
let sum = 0;
for (let pass = 0; pass < 1000; pass++) {
  for (let i = 0; i < 4096; i++) sum += a.left[i] < 0 ? -1 : 1;
}
console.log(sum, a.left[12]);
