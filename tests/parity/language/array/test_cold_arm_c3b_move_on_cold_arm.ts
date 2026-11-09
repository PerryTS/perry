// b2 C3b (REVIEW-b2-code R1, R9): ONE loop over a young array. A read at a
// fractional index takes the cold arm, whose getter allocates enough to
// force copying minors that move `a`. Every later read must use the moved
// array (run under PERRY_GC_PROTECT_FROMSPACE=1, a stale handle faults).
let a: number[] = [];
let junk: any[] = [];
Object.defineProperty(Array.prototype, "0.5", {
  configurable: true,
  get() {
    for (let i = 0; i < 40000; i++) junk.push({ i, s: "x" + i });
    junk = [];
    return -7;
  },
});
function fresh(n: number): void {
  const b: number[] = [];
  for (let i = 0; i < n; i++) b.push(i * 3);
  a = b;
}
function run(n: number): string {
  fresh(n);
  let hits = 0, cold = 0;
  for (let k = 0; k < n; k++) {
    if (a[k] === k * 3) hits++;
    if ((k & 15) === 0 && a[k + 0.5] === -7) cold++;
  }
  return hits + " " + cold;
}
console.log(run(256));
console.log(run(100));
delete (Array.prototype as any)["0.5"];
