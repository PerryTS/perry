// A push of `a + b` skips the per-value pointer test only when both
// operands are numbers by construction; strings, mixed and NaN-producing
// adds must still store and keep exactly what JS computes.
function nums(n: number): number[] {
  const xs: number[] = [];
  for (let i = 0; i < n; i++) for (let k = 0; k < 40; k++) xs.push(k + i);
  return xs;
}
function strs(n: number): string[] {
  const xs: string[] = [];
  for (let i = 0; i < n; i++) { const s = "s" + i; xs.push(s + "-" + i); }
  return xs;
}
function mixed(n: number, extra: any): any[] {
  const xs: any[] = [];
  for (let i = 0; i < n; i++) xs.push(i + extra);
  return xs;
}
function nans(n: number): number[] {
  const xs: number[] = [];
  let inf = Infinity;
  for (let i = 0; i < n; i++) { const d = inf - inf; xs.push(d + i); xs.push((i * 0.5) + (i / 3)); }
  return xs;
}
let keep: any[] = [];
for (let r = 0; r < 30; r++) {
  const a = nums(20), b = strs(50), c = mixed(30, r % 2 ? "x" : 1), d = nans(25);
  keep.push(a, b, c, d);
  if (keep.length > 40) keep = keep.slice(20);
}
const a = nums(3), b = strs(3), c = mixed(4, "x"), d = nans(3);
console.log(a.length, a[0], a[119], b.join(","), c.join(","), d.map(String).join(","));
console.log(keep.length, keep[keep.length - 3].join(",").length);
