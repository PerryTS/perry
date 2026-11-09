// b2 C10 (REVIEW-b2-code R1): after a slow-arm getter rebinds or moves `a`,
// an inner counted loop over `a` (its own tier: cached length, packed or
// validated receiver) must use the current array, never a stale handle.
let a: number[] = [];
let mode = 0;
let junk: any[] = [];
Object.defineProperty(Array.prototype, "1.5", {
  configurable: true,
  get() {
    if (mode === 0) { const b: number[] = []; for (let i = 0; i < 48; i++) b.push(i * 2); a = b; }
    else { for (let i = 0; i < 30000; i++) junk.push({ i }); junk = []; }
    return -1;
  },
});
function run(n: number): string {
  let hits = 0, inner = 0;
  for (let k = 0; k < n; k++) {
    if (a[k & 7] === (k & 7)) hits++;
    if (k === 3 && a[k / 2] === -1) hits += 100;
    for (let i = 0; i < a.length; i++) if (a[i] === 10) inner++;
  }
  return hits + " " + inner + " " + a.length;
}
function fresh(): void {
  const b: number[] = [];
  for (let i = 0; i < 32; i++) b.push(i);
  a = b;
}
for (mode = 0; mode < 2; mode++) {
  fresh();
  console.log(mode, run(8));
}
delete (Array.prototype as any)["1.5"];
