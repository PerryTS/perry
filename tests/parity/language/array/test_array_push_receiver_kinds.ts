// The inline `push` tier behind a `number[]` annotation, given values that are no Array. The annotation proves
// nothing about the value: `null`, `undefined`, a Number, a boolean, a String, a BigInt, a Symbol behind it must
// throw the call's TypeError (`x.push is not a function`), an object with a `push` of its own must run it, a
// Proxy of an Array must append through it. Before, the tier read the header below the value's low bits (SIGSEGV
// for every primitive) or handed them to the runtime as an address. The four forms: a parameter (P), a class field
// (F), a module binding (G) and a local (L), each in value and statement position.
//
// Controls: `plainPush` and `Stack.add` only ever see Arrays born for them, so the facts prove the site and the
// tier keeps its exact code (no receiver test); `owned` pushes onto a literal born in the function.
//
// Map / Set entries behind a `Map` / `Set` annotation given a cell of another kind (an Array, a Date, a closure,
// the other collection) read and WROTE that cell as a collection; the rows print the receiver afterwards, which
// must be unchanged.
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
class Q { q: number = 1; }
class WithPush { items: number[] = []; push(v: number): string { this.items.push(v); return "own" + v; } }
const KINDS = ["null", "undef", "int", "dbl", "nan", "true", "hstr", "sstr", "bigint", "sym", "obj", "cls",
  "withpush", "map", "u8", "proxy", "fn", "arr", "date"];
function mk(k: string): any {
  switch (k) {
    case "null": return JSON.parse("null");
    case "undef": return JSON.parse("[]")[0];
    case "int": return JSON.parse("5");
    case "dbl": return JSON.parse("1.5");
    case "nan": return JSON.parse("1") * NaN;
    case "true": return JSON.parse("true");
    case "hstr": return JSON.parse("\"abcdefghijklmnopqrstuvwxyz0123456789\"");
    case "sstr": return JSON.parse("\"ab\"");
    case "bigint": return BigInt(JSON.parse("5"));
    case "sym": return Symbol("s");
    case "obj": return { length: 1, 0: "p" };
    case "cls": return new Q();
    case "withpush": return new WithPush();
    case "map": { const m = new Map<any, any>(); m.set(0, "m0"); return m; }
    case "u8": { const a = new Uint8Array(2); a[0] = 7; return a; }
    case "proxy": return new Proxy(JSON.parse("[7]"), {});
    case "fn": return function f(x: any): any { return x; };
    case "arr": return JSON.parse("[7, 8]");
    case "date": return new Date(0);
  }
  return undefined;
}
function row(label: string, v: any, f: () => any): void {
  let r: string;
  try { r = show(f()); } catch (e: any) { r = "THROW " + (e && e.name); }
  console.log(label + " -> " + r + " | " + show(v));
}

function pushP(a: number[]): any { return a.push(7); }
function pushPS(a: number[]): number { a.push(7); return 0; }
class H { k: number[] = []; run(): any { return this.k.push(7); } stmt(): number { this.k.push(7); return 0; } }
let G: number[] = [];
function pushG(): any { return G.push(7); }
function pushGS(): number { G.push(7); return 0; }
function pushL(v: any): any { const a: number[] = v; return a.push(7); }
function pushLS(v: any): number { const a: number[] = v; a.push(7); return 0; }
function pushNum(a: number[], x: number, y: number): any { return a.push(x + y); }

for (const k of KINDS) {
  { const v = mk(k); row("P " + k, v, () => pushP(v)); }
  { const v = mk(k); row("PS " + k, v, () => pushPS(v)); }
  { const v = mk(k); const h = new H(); h.k = v; row("F " + k, v, () => h.run()); }
  { const v = mk(k); const h = new H(); h.k = v; row("FS " + k, v, () => h.stmt()); }
  { const v = mk(k); G = v; row("G " + k, v, () => pushG()); }
  { const v = mk(k); G = v; row("GS " + k, v, () => pushGS()); }
  { const v = mk(k); row("L " + k, v, () => pushL(v)); }
  { const v = mk(k); row("LS " + k, v, () => pushLS(v)); }
  { const v = mk(k); row("N " + k, v, () => pushNum(v, 3, 4)); }
}

// Controls: proven sites (their own Array births) and an owned literal.
function plainPush(a: number[], x: number): number { return a.push(x); }
class Stack { items: number[] = []; add(x: number): number { return this.items.push(x); } }
function owned(n: number): number[] { const out: number[] = []; for (let i = 0; i < n; i++) out.push(i * i); return out; }
{
  const a: number[] = [];
  let s = 0;
  for (let i = 0; i < 5; i++) s += plainPush(a, i);
  console.log("plainPush", s, show(a));
  const st = new Stack();
  for (let i = 0; i < 4; i++) st.add(i + 10);
  console.log("Stack", show(st.items));
  console.log("owned", show(owned(6)));
}

// Map / Set entries on a cell of another kind: no write into it, no crash.
function mapSet(m: Map<number, string>): any { m.set(5, "x"); return m.size; }
function mapGet(m: Map<number, string>): any { return m.get(0); }
function setAdd(s: Set<number>): any { s.add(5); return s.size; }
function setHas(s: Set<number>): any { return s.has(0); }
for (const k of ["arr", "date", "fn", "u8", "proxy"]) {
  { const v = mk(k); try { mapSet(v); } catch (e) { } console.log("mapSet " + k + " | " + show(v)); }
  { const v = mk(k); try { mapGet(v); } catch (e) { } console.log("mapGet " + k + " | " + show(v)); }
  { const v = mk(k); try { setAdd(v); } catch (e) { } console.log("setAdd " + k + " | " + show(v)); }
  { const v = mk(k); try { setHas(v); } catch (e) { } console.log("setHas " + k + " | " + show(v)); }
}
{ const v = mk("map"); try { setAdd(v); } catch (e) { } console.log("setAdd map | " + show(v)); }
{ const v = new Set<number>(); v.add(1); try { mapSet(v as any); } catch (e) { } console.log("mapSet set | " + show(v)); }
{ const m = new Map<number, string>(); m.set(0, "z"); console.log("mapReal", mapSet(m), mapGet(m)); }
{ const s = new Set<number>(); s.add(0); console.log("setReal", setAdd(s), setHas(s)); }
