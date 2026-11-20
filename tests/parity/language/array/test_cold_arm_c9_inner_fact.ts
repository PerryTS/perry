// b2 C9: an inner counted loop installs its own facts (the cached length of
// `b`) inside an outer covered loop; the outer reads inside it keep the
// ordinary route and still see a getter's reassignment of `a`.
let a: number[] = [1, 2, 3, 4, 5, 6, 7, 8];
const b: number[] = [3, 1, 4, 1, 5, 9, 2, 6];
Object.defineProperty(Array.prototype, "2.5", {
  configurable: true,
  get() { a = [10, 20, 30, 40, 50, 60, 70, 80]; return 0; },
});
function run(n: number): string {
  let hits = 0, big = 0;
  for (let k = 0; k < n; k++) {
    if (a[k] === k + 1) hits++;
    for (let j = 0; j < b.length; j++) {
      if (b[j] === 1 && a[(k + j) & 7] === 10 * (((k + j) & 7) + 1)) big++;
      if (k === 2 && j === 0 && a[k + 0.5] === 0) hits += 100;
    }
  }
  return hits + " " + big;
}
console.log(run(8));
delete (Array.prototype as any)["2.5"];
