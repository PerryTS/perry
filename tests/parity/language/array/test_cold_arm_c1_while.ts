// b2 C1 in `while` and `do..while` form (REVIEW-b2-code R9).
let a: number[] = [1, 2, 3, 4, 5, 6, 7, 8];
const other: number[] = [100, 200, 300, 400, 500, 600, 700, 800];
Object.defineProperty(Array.prototype, "1.5", {
  configurable: true,
  get() { a = other; return -1; },
});
function w(n: number): string {
  let old = 0, fresh = 0, seen = 0, k = 0;
  while (k < n) {
    if (a[k] === k + 1) old++;
    if (a[k] === (k + 1) * 100) fresh++;
    if (k === 3 && a[k / 2] === -1) seen++;
    k++;
  }
  return old + " " + fresh + " " + seen;
}
function d(n: number): string {
  let old = 0, fresh = 0, seen = 0, k = 0;
  do {
    if (a[k] === k + 1) old++;
    if (a[k] === (k + 1) * 100) fresh++;
    if (k === 3 && a[k / 2] === -1) seen++;
    k++;
  } while (k < n);
  return old + " " + fresh + " " + seen;
}
const keep = a;
console.log(w(8));
a = keep;
console.log(d(8));
delete (Array.prototype as any)["1.5"];
