// By-name Array method sites and the whole-program verdict. A site whose every receiver value is an Array with
// the realm's method (`plain*`, `Bag`) runs the built-in with no test of its receiver; a site that also sees an
// Array with an own method, an own prototype, a subclass override, an ordinary object or `null` keeps its guard
// and runs what the value holds. `calls` counts the user methods that ran.
let calls = 0;
function OWN(this: any, ...args: any[]): string { calls++; return "OWN(" + args.length + ")"; }
function t(label: string, f: () => any): void {
  const c0 = calls; let r: string;
  try { const v = f(); r = Array.isArray(v) ? "[" + v.join(",") + "]" : typeof v + ":" + String(v); } catch (e: any) { r = "THROW " + (e && e.name); }
  console.log(label + " -> " + r + " c=" + (calls - c0));
}
function mk(): number[] { const a: number[] = []; for (let i = 0; i < 6; i++) a[i] = (i * 7) % 5; return a; }
// The proven sites' Arrays have a birth of their own: the facts know an Array by where it was born.
function pl(): number[] { const a: number[] = []; for (let i = 0; i < 6; i++) a[i] = (i * 7) % 5; return a; }

// Proven sites: only plain Arrays reach them.
function plainIndexOf(a: number[]): number { return a.indexOf(3); }
function plainIncludes(a: number[]): boolean { return a.includes(4); }
function plainSlice(a: number[]): number[] { return a.slice(1, 4); }
function plainSort(a: number[]): number[] { return a.sort((x, y) => x - y); }
function plainJoin(a: number[]): string { return a.join("+"); }
function plainFill(a: number[]): number[] { return a.fill(9, 1, 3); }
function plainPop(a: number[]): any { return a.pop(); }
function plainShift(a: number[]): any { return a.shift(); }
function plainMap(a: number[]): number[] { return a.map((x) => x * 2); }
function plainReduce(a: number[]): number { return a.reduce((p, x) => p + x, 0); }
class Bag {
  items: number[] = pl();
  has(v: number): boolean { return this.items.includes(v); }
  where(v: number): number { return this.items.indexOf(v); }
  part(): number[] { return this.items.slice(2); }
  add(v: number): number { return this.items.push(v); }
  take(): any { return this.items.pop(); }
  sorted(): number[] { return this.items.slice().sort((x, y) => y - x); }
}
t("plain indexOf", () => plainIndexOf(pl()));
t("plain includes", () => plainIncludes(pl()));
t("plain slice", () => plainSlice(pl()));
t("plain sort", () => plainSort(pl()));
t("plain join", () => plainJoin(pl()));
t("plain fill", () => plainFill(pl()));
t("plain pop", () => plainPop(pl()));
t("plain shift", () => plainShift(pl()));
t("plain map", () => plainMap(pl()));
t("plain reduce", () => plainReduce(pl()));
{
  const b = new Bag();
  t("bag has", () => b.has(4)); t("bag where", () => b.where(2)); t("bag part", () => b.part());
  t("bag add", () => b.add(11)); t("bag take", () => b.take()); t("bag sorted", () => b.sorted());
  let s = 0; for (let i = 0; i < 2000; i++) { b.add(i); if (b.has(i)) s += b.where(i); s += b.take(); }
  console.log("bag loop", s, b.items.length, calls);
}

// Unproven sites: the same shapes, reached by a value whose method is not the built-in.
function anyIndexOf(a: number[]): any { return a.indexOf(3); }
function anyIncludes(a: number[]): any { return a.includes(4); }
function anySlice(a: number[]): any { return a.slice(1, 4); }
function anySort(a: number[]): any { return a.sort(); }
function anyJoin(a: number[]): any { return a.join("+"); }
function anyFill(a: number[]): any { return a.fill(9, 1, 3); }
function anyShift(a: number[]): any { return a.shift(); }
function anyMap(a: number[]): any { return a.map((x) => x * 2); }
class Sub extends Array<number> { indexOf(): any { calls++; return "SUB"; } join(): any { calls++; return "SUB"; } }
class Holder { k: number[] = mk(); find(): any { return this.k.indexOf(3); } cut(): any { return this.k.slice(1, 4); } glue(): any { return this.k.join("+"); } }
const KINDS: [string, () => any][] = [
  ["plain", () => mk()],
  ["own", () => { const a: any = mk(); for (const m of ["indexOf", "includes", "slice", "sort", "join", "fill", "shift", "map"]) a[m] = OWN; return a; }],
  ["proto", () => { const a: any = mk(); const p: any = Object.create(Array.prototype); for (const m of ["indexOf", "includes", "slice", "sort", "join", "fill", "shift", "map"]) p[m] = OWN; Object.setPrototypeOf(a, p); return a; }],
  ["sub", () => { const s: any = new Sub(); s[0] = 3; s[1] = 1; return s; }],
  ["object", () => ({ length: 0, indexOf: OWN, includes: OWN, slice: OWN, sort: OWN, join: OWN, fill: OWN, shift: OWN, map: OWN })],
  ["bare", () => ({ length: 0 })],
  ["null", () => JSON.parse("null")],
  ["undefined", () => JSON.parse("[]")[0]],
];
for (const [kind, make] of KINDS) {
  t(kind + " indexOf", () => anyIndexOf(make()));
  t(kind + " includes", () => anyIncludes(make()));
  t(kind + " slice", () => anySlice(make()));
  t(kind + " sort", () => anySort(make()));
  t(kind + " join", () => anyJoin(make()));
  t(kind + " fill", () => anyFill(make()));
  t(kind + " shift", () => anyShift(make()));
  t(kind + " map", () => anyMap(make()));
  const h = new Holder(); h.k = make();
  t(kind + " field indexOf", () => h.find());
  t(kind + " field slice", () => h.cut());
  t(kind + " field join", () => h.glue());
}
console.log("calls", calls);
