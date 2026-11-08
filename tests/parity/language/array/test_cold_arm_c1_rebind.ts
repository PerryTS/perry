// b2 C1: a getter on the slow arm (reached by a fractional index) reassigns
// the module-global receiver; every later read in the loop sees the new array.
let a: number[] = [1, 2, 3, 4, 5, 6, 7, 8];
const other: number[] = [100, 200, 300, 400, 500, 600, 700, 800];
Object.defineProperty(Array.prototype, "1.5", {
  configurable: true,
  get() { a = other; return -1; },
});
let out: number[] = [];
for (let k = 0; k < 8; k++) {
  out.push(a[k]);
  if (k === 3) out.push(a[k / 2]);
}
console.log(out.join(" "));
delete (Array.prototype as any)["1.5"];
