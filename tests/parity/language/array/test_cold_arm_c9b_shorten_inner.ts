// b2 C9b (REVIEW-b2-code R4): a slow-arm getter shortens `b` while an inner
// counted loop over `b` runs; the inner loop's bound and reads follow it.
let a: number[] = [1, 2, 3, 4, 5, 6, 7, 8];
let b: number[] = [3, 1, 4, 1, 5, 9, 2, 6];
Object.defineProperty(Array.prototype, "2.5", {
  configurable: true,
  get() { b.length = 2; return 0; },
});
function run(n: number): string {
  let hits = 0, seen = 0, und = 0;
  for (let k = 0; k < n; k++) {
    if (a[k] === k + 1) hits++;
    for (let j = 0; j < b.length; j++) {
      if (b[j] === undefined) und++;
      else seen++;
      if (k === 2 && j === 0 && a[k + 0.5] === 0) hits += 100;
    }
  }
  return hits + " " + seen + " " + und + " " + b.length;
}
console.log(run(8));
delete (Array.prototype as any)["2.5"];
