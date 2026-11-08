// b2 C3: in a covered loop, a getter on the slow arm allocates enough to
// force collections that move the receiver; reads stay exact.
let a: number[] = [];
for (let i = 0; i < 64; i++) a.push(i * 3);
let junk: any[] = [];
Object.defineProperty(Array.prototype, "0.5", {
  configurable: true,
  get() {
    for (let i = 0; i < 20000; i++) junk.push({ i, s: "x" + i });
    junk = [];
    return -7;
  },
});
let hits = 0, cold = 0;
for (let r = 0; r < 50; r++) {
  for (let k = 0; k < 64; k++) if (a[k] === k * 3) hits++;
  if (a[(r & 1) === 0 ? 0.5 : 1] === -7) cold++;
}
console.log(hits, cold);
delete (Array.prototype as any)["0.5"];
