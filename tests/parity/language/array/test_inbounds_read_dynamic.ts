// The in-bounds read and code the whole-program facts do not see: an Array
// handed to `new Function` or reached by `eval` may be shortened there
// between the guard and the read, so its reads keep their bound test.
function gen(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 3 + 1);
  return out;
}
function show(v: unknown): string {
  return v === undefined ? "U" : String(v);
}
function viaFunction(n: number): string {
  const xs = gen(n);
  const cut = new Function("a", "a.pop(); a.pop();") as (a: number[]) => void;
  let out = "";
  for (let i = 0; i < xs.length; i++) {
    if (i === 1) cut(xs);
    out += show(xs[i]) + ",";
  }
  return out;
}
function viaEval(n: number): string {
  const xs = gen(n);
  let out = "";
  for (let i = 0; i < xs.length; i++) {
    if (i === 2) eval("xs.length = 1");
    out += show(xs[i]) + ",";
  }
  return out;
}
function viaGlobal(n: number): string {
  const xs = gen(n);
  (globalThis as any).shared = xs;
  const cut = new Function("globalThis.shared.length = 2;") as () => void;
  let out = "";
  for (let i = 0; i < xs.length; i++) {
    if (i === 2) cut();
    out += show(xs[i]) + ",";
  }
  return out;
}
console.log("function", viaFunction(5));
console.log("eval", viaEval(5));
console.log("global", viaGlobal(5));
