// By-name Array method sites behind a `number[]` annotation given values that are no Array. node looks the method
// up on the value: a Number, a boolean, a BigInt, a Symbol, a Map, a Set, a Date or a function has no `at` /
// `indexOf` / `slice` / `pop` / `join` / `includes` / `concat` (TypeError `x.<m> is not a function`), a String and a
// typed array have their own. Before, a Number and every heap cell that is no Array or ordinary object ran the
// Array built-in on it as an array-like. Controls: real Arrays, an ordinary object with its own methods.
function show(r: any): string {
  const t = typeof r;
  if (r === null) return "null";
  if (t === "bigint") return "bigint:" + String(r);
  if (t === "symbol") return "symbol";
  if (t === "function") return "function";
  if (t !== "object") return t + ":" + String(r);
  if (r instanceof Map) return "Map" + r.size;
  if (r instanceof Set) return "Set" + r.size;
  if (r instanceof Date) return "Date" + r.getTime();
  if (Array.isArray(r)) return "A[" + r.join(",") + "]";
  if (ArrayBuffer.isView(r)) return "TA[" + Array.from(r as any).join(",") + "]";
  let s = "O{";
  for (const k of Object.keys(r)) s += k + ":" + String(r[k]) + ",";
  return s + "}";
}
function mk(k: string): any {
  switch (k) {
    case "int": return JSON.parse("5");
    case "dbl": return JSON.parse("1.5");
    case "nan": return JSON.parse("1") * NaN;
    case "true": return JSON.parse("true");
    case "bigint": return BigInt(JSON.parse("5"));
    case "sym": return Symbol("s");
    case "map": { const m = new Map<any, any>(); m.set(0, "m0"); return m; }
    case "set": { const s = new Set<any>(); s.add(8); return s; }
    case "date": return new Date(0);
    case "fn": return function f(x: any): any { return x; };
    case "u8": { const a = new Uint8Array(2); a[0] = 7; a[1] = 8; return a; }
    case "str": return JSON.parse("\"xyz8\"");
    case "own": return { length: 1, 0: 7, at() { return "OWN-at"; }, indexOf() { return "OWN-indexOf"; }, pop() { return "OWN-pop"; } };
    case "arr": return JSON.parse("[7, 8]");
  }
  return undefined;
}
function row(label: string, v: any, f: () => any): void {
  let r: string;
  try { r = show(f()); } catch (e: any) { r = "THROW " + (e && e.name); }
  console.log(label + " -> " + r + " | " + show(v));
}
function atP(a: number[]): any { return a.at(0); }
function indexOfP(a: number[]): any { return a.indexOf(8); }
function includesP(a: number[]): any { return a.includes(8); }
function sliceP(a: number[]): any { return a.slice(0, 1); }
function popP(a: number[]): any { return a.pop(); }
function joinP(a: number[]): any { return a.join("-"); }
class H { k: number[] = []; at(): any { return this.k.at(0); } idx(): any { return this.k.indexOf(8); } }
let G: number[] = [];
function atG(): any { return G.at(0); }
function sliceG(): any { return G.slice(0, 1); }
for (const k of ["int", "dbl", "nan", "true", "bigint", "sym", "map", "set", "date", "fn", "u8", "str", "own", "arr"]) {
  { const v = mk(k); row("at " + k, v, () => atP(v)); }
  { const v = mk(k); row("indexOf " + k, v, () => indexOfP(v)); }
  { const v = mk(k); row("includes " + k, v, () => includesP(v)); }
  { const v = mk(k); row("slice " + k, v, () => sliceP(v)); }
  { const v = mk(k); row("pop " + k, v, () => popP(v)); }
  { const v = mk(k); row("join " + k, v, () => joinP(v)); }
  { const v = mk(k); const h = new H(); h.k = v; row("F.at " + k, v, () => h.at()); row("F.indexOf " + k, v, () => h.idx()); }
  { const v = mk(k); G = v; row("G.at " + k, v, () => atG()); row("G.slice " + k, v, () => sliceG()); }
}
