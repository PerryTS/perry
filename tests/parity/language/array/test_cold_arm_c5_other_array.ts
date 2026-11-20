// b2 C5: two arrays in one loop; the slow-arm getter of `a` shortens `b`,
// which a counted read reads afterwards.
let a: number[] = [1, 2, 3, 4, 5, 6, 7, 8];
let b: number[] = [10, 20, 30, 40, 50, 60, 70, 80];
Object.defineProperty(Array.prototype, "2.5", {
  configurable: true,
  get() { b.length = 1; return 0; },
});
const out: any[] = [];
for (let i = 0; i < b.length; i++) {
  out.push(a[i & 7]);
  if (i === 2) out.push(a[i + 0.5]);
  out.push(b[i]);
}
console.log(out.join(" "), b.length);
delete (Array.prototype as any)["2.5"];
