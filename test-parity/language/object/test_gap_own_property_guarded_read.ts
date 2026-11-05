// A computed read behind an own-property test of the same object and key
// reads own properties only (TypeScript's `getProperty` in core.ts); the
// same read without the test reaches Object.prototype (`constructor` is the
// Object constructor, `toString` its method).
const hasOwnProperty = Object.prototype.hasOwnProperty;
function getProperty<T>(map: Record<string, T>, key: string): T | undefined {
  return hasOwnProperty.call(map, key) ? map[key] : undefined;
}
class Sym {
  n: number;
  constructor(n: number) { this.n = n; }
  size(): number { return this.n; }
}
function show(v: any): string {
  if (v === undefined) return "undefined";
  if (v instanceof Sym) return "Sym " + v.size();
  if (v === Object) return "Object constructor";
  if (typeof v === "function") return "function " + v.name;
  if (v === Object.prototype) return "Object.prototype";
  return typeof v + " " + String(v);
}
const dict: Record<string, Sym> = {};
const names = ["a", "b"];
for (let i = 0; i < names.length; i++) dict[names[i]] = new Sym(i + 1);
const keys = ["a", "x", "constructor", "toString", "b", "__proto__", "hasOwnProperty"];
const out: string[] = [];
for (const k of keys) {
  out.push(k + " unguarded " + show((dict as any)[k]));
  out.push(k + " getProperty " + show(getProperty(dict, k)));
  out.push(k + " and " + String(hasOwnProperty.call(dict, k) && dict[k].size()));
  let v = "undefined";
  if (hasOwnProperty.call(dict, k)) {
    const s = dict[k];
    v = "Sym " + s.size();
  }
  out.push(k + " if " + v);
  out.push(k + " hasOwn " + (Object.hasOwn(dict, k) ? dict[k].size() : "undefined"));
  out.push(k + " method " + (dict.hasOwnProperty(k) ? dict[k].size() : "undefined"));
  out.push(
    k + " enumerable " +
      (Object.prototype.propertyIsEnumerable.call(dict, k) ? dict[k].size() : "undefined"),
  );
}
// An own accessor: the test is true and the read runs the own getter.
let gets = 0;
const acc: any = { get total() { gets++; return 42; } };
for (const k of ["total", "valueOf"]) {
  out.push(k + " accessor " + (hasOwnProperty.call(acc, k) ? acc[k] : "undefined") + " gets=" + gets);
}
// A dictionary that really holds "constructor" as its own key.
const ctorDict: Record<string, Sym> = {};
const ck = keys[2];
ctorDict[ck] = new Sym(7);
out.push("own constructor " + show(getProperty(ctorDict, ck)) + " / other dict " + show(getProperty(dict, ck)));
// clone/extend copy own values only.
function clone<T>(o: Record<string, T>): Record<string, T> {
  const r: any = {};
  for (const id in o) {
    if (hasOwnProperty.call(o, id)) {
      r[id] = o[id];
    }
  }
  return r;
}
const inherited: any = Object.create(dict);
inherited.c = new Sym(3);
const c = clone(inherited);
out.push("clone keys " + Object.keys(c).join(",") + " c=" + show(c["c"]));
// An object key converts at the test and again at the read: the two may differ.
let flip = false;
const ok: any = { toString() { flip = !flip; return flip ? "a" : "constructor"; } };
out.push("object key " + show(hasOwnProperty.call(dict, ok) ? (dict as any)[ok] : undefined));
// A program function as the test: the read is an ordinary one.
let h: any = hasOwnProperty;
if (out.length > 3) h = function (this: any, key: string) { return key.length > 0; };
out.push("program test " + show(h.call(dict, "constructor") ? (dict as any)["constructor"] : undefined));
console.log(out.join("\n"));
