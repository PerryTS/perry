// b2 C2: a getter on the slow arm changes the receiver's kind, length,
// storage or element descriptors; later reads in the loop see each change.
let a: any[] = [];
function reset() { a = [1, 2, 3, 4, 5, 6, 7, 8]; }
let mode = 0;
Object.defineProperty(Array.prototype, "1.5", {
  configurable: true,
  get() {
    if (mode === 0) a[0] = "s";
    else if (mode === 1) a.length = 2;
    else if (mode === 2) for (let i = 0; i < 1000; i++) a.push(i);
    else Object.defineProperty(a, 3, { get() { return "acc"; }, configurable: true });
    return 0;
  },
});
for (mode = 0; mode < 4; mode++) {
  reset();
  const out: any[] = [];
  for (let k = 0; k < 6; k++) {
    out.push(a[k % 4]);
    if (k === 1) out.push(a[k + 0.5]);
  }
  console.log(mode, out.join(" "), a.length);
}
delete (Array.prototype as any)["1.5"];
