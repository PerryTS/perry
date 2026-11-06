// Charter step 5L, P6: a Number local owns no root slot, in every body kind
// (function, method, static method, closure). Each loop allocates, so
// collections run while the locals are live, and each local is fed NaN,
// -NaN, -0 and infinities through `-`, `Math.abs`, `Math.sqrt`, `0/0` and
// division, so a sign flip of a NaN is observable if a local ever held one
// outside the canonical set. `mixed` keeps a pointer-capable local beside
// them, which must stay rooted.
const SINK: any[] = [];
function churn(i: number): void {
  SINK.push({ i, s: "x" + i, a: [i, i + 1] });
  if (SINK.length > 256) SINK.length = 0;
}
function describe(x: number): string {
  if (x !== x) return "NaN";
  if (x === 0) return 1 / x < 0 ? "-0" : "0";
  return String(x);
}
class Acc {
  sum(n: number): string {
    let h: any = 0.5;
    let q: any = -0;
    let z: any = 0;
    for (let i = 0; i < n; i++) {
      churn(i);
      h = (h * 3 + i) % 1000003;
      z = 0 / 0;
      z = -z;
      q = -(z + h) + Math.abs(-z);
      h = h + (q !== q ? 1 : 0);
    }
    return describe(h) + "," + describe(q) + "," + describe(z);
  }
  static stat(n: number): string {
    let t: any = 0;
    let w: any = 1 / 0;
    for (let i = 0; i < n; i++) {
      churn(i);
      t = t - i * 2;
      w = -w;
      t = t + (w - w === w - w ? 1 : 0);
    }
    return describe(t) + "," + describe(w) + "," + describe(w - w);
  }
}
function viaClosure(n: number): string {
  const f = (m: number): string => {
    let s: any = 1;
    let r: any = 0;
    for (let i = 0; i < m; i++) {
      churn(i);
      s = (s * 7 + i) % 1000003;
      r = -Math.sqrt(-s);
      r = -r;
      s = s + (r === r ? 5 : 0);
    }
    return describe(s) + "," + describe(r);
  };
  return f(n);
}
function inFunction(n: number): string {
  let a: any = 2;
  let b: any = -0;
  for (let i = 0; i < n; i++) {
    churn(i);
    a = (a * 5 + 1) % 65521;
    b = -(b * 0);
    a = a - Math.abs(b);
  }
  return describe(a) + "," + describe(b);
}
function mixed(n: number): string {
  let a: any = 0;
  let k = 0;
  for (let i = 0; i < n; i++) {
    churn(i);
    k = k + 1;
    a = i % 7 === 0 ? "s" + i : a + 1;
  }
  return String(a) + ":" + k;
}
const N = 20000;
console.log(new Acc().sum(N));
console.log(Acc.stat(N));
console.log(viaClosure(N));
console.log(inFunction(N));
console.log(mixed(N));
