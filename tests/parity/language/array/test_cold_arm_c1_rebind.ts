// b2 C1: in a loop the cold-arm descriptor covers, a getter on the slow arm
// (reached by a fractional index) reassigns the module-global receiver;
// every later read sees the new array.
let a: number[] = [1, 2, 3, 4, 5, 6, 7, 8];
const other: number[] = [100, 200, 300, 400, 500, 600, 700, 800];
Object.defineProperty(Array.prototype, "1.5", {
  configurable: true,
  get() { a = other; return -1; },
});
function run(): string {
  let old = 0, fresh = 0, seen = 0;
  for (let k = 0; k < 8; k++) {
    if (a[k] === k + 1) old++;
    if (a[k] === (k + 1) * 100) fresh++;
    if (k === 3 && a[k / 2] === -1) seen++;
  }
  return old + " " + fresh + " " + seen;
}
console.log(run());
delete (Array.prototype as any)["1.5"];
