function step(i: number): number {
  try { if (i >= 0) return i; return -1; } catch { return -2; }
}
let sum = 0;
for (let i = 0; i < 500; i++) sum += step(i);
console.log("sum=" + sum);
console.log("done");
