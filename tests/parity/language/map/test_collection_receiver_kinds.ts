// Folded Map / Set method sites (`m.get(k)`, `m.set(k, v)`, `m.has(k)`, `m.delete(k)`, `s.add(v)`, `s.has(v)`,
// `s.delete(v)`) behind a `Map` / `Set` annotation given values that are no such collection. node performs the real
// `Get` + `Call`: a TypeError where the value has no such member, the value's own member otherwise (a Map behind
// `Set<T>` answers `has` / `delete` with `Map.prototype`, an object with its own `get` runs it, a typed array runs
// `%TypedArray%.prototype.set`). Before, the collection built-in refused the cell and answered `undefined` / `false`.
// Controls: real Maps and Sets, subclass instances, a field and a module binding that only hold real collections.
class SubMap extends Map<any, any> {
  get(k: any): any {
    return "sub:" + super.get(k);
  }
}
function mk(k: string): any {
  switch (k) {
    case "map": { const m = new Map<any, any>(); m.set(1, "one"); return m; }
    case "set": { const s = new Set<any>(); s.add(1); return s; }
    case "submap": { const m = new SubMap(); m.set(1, "one"); return m; }
    case "weakmap": return new WeakMap();
    case "arr": return [1, 2];
    case "obj": return { a: 1 };
    case "own": return { get(k: any) { return "own get " + k; }, has() { return "own has"; }, add() { return "own add"; } };
    case "date": return new Date(0);
    case "fn": return function f() { return 1; };
    case "u8": return new Uint8Array(4);
    case "ab": return new ArrayBuffer(8);
    case "str": return "s";
    case "num": return 5;
    case "sym": return Symbol("s");
  }
  return null;
}
function show(r: any): string {
  if (r === null) return "null";
  const t = typeof r;
  if (t === "object" || t === "function") {
    if (r instanceof Map) return "Map" + r.size;
    if (r instanceof Set) return "Set" + r.size;
    return t;
  }
  return t + ":" + String(r);
}
function mGet(m: Map<number, string>): any { return m.get(1); }
function mSet(m: Map<number, string>): any { return m.set(2, "two"); }
function mHas(m: Map<number, string>): any { return m.has(1); }
function mDel(m: Map<number, string>): any { return m.delete(1); }
function sAdd(s: Set<number>): any { return s.add(3); }
function sHas(s: Set<number>): any { return s.has(1); }
function sDel(s: Set<number>): any { return s.delete(1); }
const routes: [string, (v: any) => any][] = [
  ["m.get", mGet], ["m.set", mSet], ["m.has", mHas], ["m.delete", mDel],
  ["s.add", sAdd], ["s.has", sHas], ["s.delete", sDel],
];
for (const k of ["map", "set", "submap", "weakmap", "arr", "obj", "own", "date", "fn", "u8", "ab", "str", "num", "sym"]) {
  let line = k + ":";
  for (const [name, f] of routes) {
    let r: string;
    try {
      r = show(f(mk(k)));
    } catch (e: any) {
      r = "THROW " + e.name;
    }
    line += " " + name + "=" + r;
  }
  console.log(line);
}
// A field and a module binding that only ever hold real collections.
class Holder {
  table: Map<string, number> = new Map<string, number>();
  seen: Set<number> = new Set<number>();
  run(k: string): string {
    this.table.set(k, k.length);
    this.seen.add(k.length);
    return String(this.table.get(k)) + "/" + String(this.seen.has(k.length)) + "/" + String(this.table.has("zz"));
  }
}
const G: Map<string, number> = new Map<string, number>();
function useG(): string {
  G.set("g", 7);
  return String(G.get("g")) + "/" + String(G.delete("g")) + "/" + String(G.has("g"));
}
console.log(new Holder().run("abc"), useG());
