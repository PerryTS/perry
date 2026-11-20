// A literal a callee grows keeps the length the callee gives it: its reads after
// the assignment see the pushes, in the caller and in every summary instance.
function fill(out: number[], n: number): void {
  for (let i = 0; i < n; i++) out.push(out.length * 2);
}
function count(a: number[]): number {
  let n = 0;
  for (let i = 0; i < a.length; i++) n++;
  return n;
}
function grown(xs: number[]): number {
  let s = 0;
  const other: number[] = [5, 6, 7];
  for (const v of xs) { s += v + other.length; fill(other, 1); }
  const n = other.length;
  return s + n;
}
function kept(xs: number[]): number {
  let s = 0;
  const other: number[] = [5, 6, 7];
  for (const v of xs) { s += v + other.length + count(other); }
  const n = other.length;
  return s + n;
}
console.log(grown([1, 4, 7, 10, 13]), grown([]), kept([1, 2]), kept([]));
