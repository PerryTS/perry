// b2 C3: in a covered loop, a getter on the slow arm allocates enough to
// force collections that move the receiver (allocated fresh, so it is still
// young when the loop starts); reads stay exact.
let a: number[] = [];
let junk: any[] = [];
Object.defineProperty(Array.prototype, "0.5", {
  configurable: true,
  get() {
    for (let i = 0; i < 20000; i++) junk.push({ i, s: "x" + i });
    junk = [];
    return -7;
  },
});
function fresh(): void {
  const b: number[] = [];
  for (let i = 0; i < 64; i++) b.push(i * 3);
  a = b;
}
function run(rounds: number, n: number): string {
  fresh();
  let hits = 0, cold = 0;
  for (let r = 0; r < rounds; r++) {
    for (let k = 0; k < n; k++) if (a[k] === k * 3) hits++;
    if (a[(r & 1) === 0 ? 0.5 : 1] === -7) cold++;
  }
  return hits + " " + cold;
}
console.log(run(50, 64));
console.log(run(7, 64));
delete (Array.prototype as any)["0.5"];
