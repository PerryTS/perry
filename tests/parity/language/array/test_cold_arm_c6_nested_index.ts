// b2 C6: a[a[k]]: the inner read's slow-arm getter reassigns `a`; the outer
// read uses the base evaluated before its index (the old array).
let a: number[] = [1, 0.5, 3, 2, 0, 1, 2, 3];
const other: number[] = [9, 9, 9, 9, 9, 9, 9, 9];
Object.defineProperty(Array.prototype, "0.5", {
  configurable: true,
  get() { a = other; return 4; },
});
const out: number[] = [];
for (let k = 0; k < 4; k++) out.push(a[a[k]]);
console.log(out.join(" "));
delete (Array.prototype as any)["0.5"];
