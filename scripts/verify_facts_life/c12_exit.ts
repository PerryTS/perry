// a frame local (a closure the census proves frame) whose statement list is
// left only by `continue` / `break`: its deaths execute at the exits (G2),
// never at the list end.
function sum(n: number, m: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const f = (k: number) => k * m;
    s += f(i);
    s += f(1);
    if (i === n - 1) break;
    continue;
  }
  return s;
}
let t = 0;
for (let k = 0; k < 1000; k++) { t += sum(10, k % 7); }
console.log(t);
