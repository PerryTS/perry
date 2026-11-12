// `a.m(..)` is `Call(Get(a, "m"), a, [..])`: a tier that runs the realm's
// `Array.prototype.m` by name must know that `m` IS that builtin on its
// receiver. `push` has its own fixture (test_array_push_method_tiers); this one
// covers every other Array method the compiler lowers by name. The receivers
// are all annotated `number[]`:
//   own    an Array with an own data property of the name
//   acc    an Array with an own accessor of the name
//   proto  an Array whose own [[Prototype]] holds the name (a compiled program
//          may not replace a member of `Array.prototype` itself)
//   sub    an Array subclass that overrides the name
//   fake   an object that is no Array, with its own method of the name
//   none   an array-like object without the method: node throws TypeError
// and the negative controls, where the builtin must still run:
//   plain  a plain Array
//   named  an Array with an unrelated own property
//   lent   an Array whose own property of the name IS the builtin
// (`toSpliced` is left out of `lent`: the dispatcher's by-name `toSpliced`
// answers an Array that has a named property with an empty object, before
// and after this fixture's change; a runtime body, not a tier.)
// Each reaches the site through a parameter (P), a class field (F), a module
// binding (G) and a local (L), in value position and, for the parameter, in
// statement position (S). Every line ends with the number of user-method
// calls the site made.

let sink = 0;
let calls = 0;
function OWN(this: any, ...args: any[]): any {
  calls++;
  return "OWN(" + args.length + ")";
}
function st(a: any): string {
  let s = Array.isArray(a) ? "A[" : "O[";
  const n = a.length;
  for (let i = 0; i < n; i++) s += String(a[i]) + ",";
  return s + "]";
}
function show(r: any): string {
  if (r !== null && typeof r === "object") {
    if (typeof r.next === "function") {
      let s = "I[";
      for (const v of r) s += String(v) + ";";
      return s + "]";
    }
    return st(r);
  }
  return typeof r + ":" + String(r);
}
function T(label: string, f: () => any, a: any): void {
  const c0 = calls;
  let r: string;
  try {
    r = show(f());
  } catch (e: any) {
    r = "THROW " + (e && e.name);
  }
  console.log(label + " -> " + r + " | " + st(a) + " c=" + (calls - c0));
}
const KEY: string[] = ["pop", "shift", "unshift", "indexOf", "lastIndexOf", "includes", "slice", "splice", "concat", "join", "reverse", "fill", "at", "map", "filter", "forEach", "some", "every", "find", "findIndex", "reduce", "sort", "copyWithin", "entries", "findLast", "findLastIndex", "flat", "flatMap", "keys", "reduceRight", "toReversed", "toSorted", "toSpliced", "values", "with"];
function mk(): number[] {
  const a: number[] = [];
  a[0] = 3;
  a[1] = 1;
  a[2] = 2;
  return a;
}
function mkOwn(i: number): number[] {
  const a = mk();
  (a as any)[KEY[i]] = OWN;
  return a;
}
function mkAcc(i: number): number[] {
  const a = mk();
  Object.defineProperty(a, KEY[i], { get() { return OWN; }, configurable: true });
  return a;
}
function mkProto(i: number): number[] {
  const a = mk();
  const p: any = Object.create(Array.prototype);
  p[KEY[i]] = OWN;
  Object.setPrototypeOf(a, p);
  return a;
}
function mkNamed(i: number): number[] {
  const a = mk();
  (a as any).tag = "t" + i;
  return a;
}
function mkLent(i: number): number[] {
  const a = mk();
  (a as any)[KEY[i]] = (Array.prototype as any)[KEY[i]];
  return a;
}
class Sub extends Array<number> {
  pop(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  shift(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  unshift(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  indexOf(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  lastIndexOf(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  includes(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  slice(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  splice(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  concat(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  join(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  reverse(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  fill(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  at(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  map(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  filter(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  forEach(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  some(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  every(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  find(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  findIndex(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  reduce(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  sort(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  copyWithin(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  entries(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  findLast(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  findLastIndex(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  flat(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  flatMap(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  keys(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  reduceRight(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  toReversed(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  toSorted(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  toSpliced(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  values(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  with(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
}
function mkSub(i: number): number[] {
  const s: any = new Sub();
  s[0] = 3;
  s[1] = 1;
  s[2] = 2;
  return s;
}
function mkFake(i: number): number[] {
  const o: any = { length: 2, 0: 3, 1: 1 };
  o[KEY[i]] = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  return o;
}
function mkNone(i: number): number[] {
  const o: any = { length: 2, 0: 3, 1: 1 };
  return o;
}
let G: number[] = mk();

function P_pop(a: number[]): any { return a.pop(); }
function S_pop(a: number[]): void { a.pop(); }
class F_pop { k: number[] = mk(); run(): any { return this.k.pop(); } }
function G_pop(): any { return G.pop(); }
function P_shift(a: number[]): any { return a.shift(); }
function S_shift(a: number[]): void { a.shift(); }
class F_shift { k: number[] = mk(); run(): any { return this.k.shift(); } }
function G_shift(): any { return G.shift(); }
function P_unshift(a: number[]): any { return a.unshift(7); }
function S_unshift(a: number[]): void { a.unshift(7); }
class F_unshift { k: number[] = mk(); run(): any { return this.k.unshift(7); } }
function G_unshift(): any { return G.unshift(7); }
function P_indexOf(a: number[]): any { return a.indexOf(1); }
function S_indexOf(a: number[]): void { a.indexOf(1); }
class F_indexOf { k: number[] = mk(); run(): any { return this.k.indexOf(1); } }
function G_indexOf(): any { return G.indexOf(1); }
function P_lastIndexOf(a: number[]): any { return a.lastIndexOf(1); }
function S_lastIndexOf(a: number[]): void { a.lastIndexOf(1); }
class F_lastIndexOf { k: number[] = mk(); run(): any { return this.k.lastIndexOf(1); } }
function G_lastIndexOf(): any { return G.lastIndexOf(1); }
function P_includes(a: number[]): any { return a.includes(1); }
function S_includes(a: number[]): void { a.includes(1); }
class F_includes { k: number[] = mk(); run(): any { return this.k.includes(1); } }
function G_includes(): any { return G.includes(1); }
function P_slice(a: number[]): any { return a.slice(0, 2); }
function S_slice(a: number[]): void { a.slice(0, 2); }
class F_slice { k: number[] = mk(); run(): any { return this.k.slice(0, 2); } }
function G_slice(): any { return G.slice(0, 2); }
function P_splice(a: number[]): any { return a.splice(0, 1); }
function S_splice(a: number[]): void { a.splice(0, 1); }
class F_splice { k: number[] = mk(); run(): any { return this.k.splice(0, 1); } }
function G_splice(): any { return G.splice(0, 1); }
function P_concat(a: number[]): any { return a.concat([9]); }
function S_concat(a: number[]): void { a.concat([9]); }
class F_concat { k: number[] = mk(); run(): any { return this.k.concat([9]); } }
function G_concat(): any { return G.concat([9]); }
function P_join(a: number[]): any { return a.join("-"); }
function S_join(a: number[]): void { a.join("-"); }
class F_join { k: number[] = mk(); run(): any { return this.k.join("-"); } }
function G_join(): any { return G.join("-"); }
function P_reverse(a: number[]): any { return a.reverse(); }
function S_reverse(a: number[]): void { a.reverse(); }
class F_reverse { k: number[] = mk(); run(): any { return this.k.reverse(); } }
function G_reverse(): any { return G.reverse(); }
function P_fill(a: number[]): any { return a.fill(5); }
function S_fill(a: number[]): void { a.fill(5); }
class F_fill { k: number[] = mk(); run(): any { return this.k.fill(5); } }
function G_fill(): any { return G.fill(5); }
function P_at(a: number[]): any { return a.at(0); }
function S_at(a: number[]): void { a.at(0); }
class F_at { k: number[] = mk(); run(): any { return this.k.at(0); } }
function G_at(): any { return G.at(0); }
function P_map(a: number[]): any { return a.map((x: number) => x + 1); }
function S_map(a: number[]): void { a.map((x: number) => x + 1); }
class F_map { k: number[] = mk(); run(): any { return this.k.map((x: number) => x + 1); } }
function G_map(): any { return G.map((x: number) => x + 1); }
function P_filter(a: number[]): any { return a.filter((x: number) => x > 1); }
function S_filter(a: number[]): void { a.filter((x: number) => x > 1); }
class F_filter { k: number[] = mk(); run(): any { return this.k.filter((x: number) => x > 1); } }
function G_filter(): any { return G.filter((x: number) => x > 1); }
function P_forEach(a: number[]): any { return a.forEach((x: number) => { sink += x; }); }
function S_forEach(a: number[]): void { a.forEach((x: number) => { sink += x; }); }
class F_forEach { k: number[] = mk(); run(): any { return this.k.forEach((x: number) => { sink += x; }); } }
function G_forEach(): any { return G.forEach((x: number) => { sink += x; }); }
function P_some(a: number[]): any { return a.some((x: number) => x > 1); }
function S_some(a: number[]): void { a.some((x: number) => x > 1); }
class F_some { k: number[] = mk(); run(): any { return this.k.some((x: number) => x > 1); } }
function G_some(): any { return G.some((x: number) => x > 1); }
function P_every(a: number[]): any { return a.every((x: number) => x > 1); }
function S_every(a: number[]): void { a.every((x: number) => x > 1); }
class F_every { k: number[] = mk(); run(): any { return this.k.every((x: number) => x > 1); } }
function G_every(): any { return G.every((x: number) => x > 1); }
function P_find(a: number[]): any { return a.find((x: number) => x > 1); }
function S_find(a: number[]): void { a.find((x: number) => x > 1); }
class F_find { k: number[] = mk(); run(): any { return this.k.find((x: number) => x > 1); } }
function G_find(): any { return G.find((x: number) => x > 1); }
function P_findIndex(a: number[]): any { return a.findIndex((x: number) => x > 1); }
function S_findIndex(a: number[]): void { a.findIndex((x: number) => x > 1); }
class F_findIndex { k: number[] = mk(); run(): any { return this.k.findIndex((x: number) => x > 1); } }
function G_findIndex(): any { return G.findIndex((x: number) => x > 1); }
function P_reduce(a: number[]): any { return a.reduce((p: number, x: number) => p + x, 0); }
function S_reduce(a: number[]): void { a.reduce((p: number, x: number) => p + x, 0); }
class F_reduce { k: number[] = mk(); run(): any { return this.k.reduce((p: number, x: number) => p + x, 0); } }
function G_reduce(): any { return G.reduce((p: number, x: number) => p + x, 0); }
function P_sort(a: number[]): any { return a.sort((x: number, y: number) => x - y); }
function S_sort(a: number[]): void { a.sort((x: number, y: number) => x - y); }
class F_sort { k: number[] = mk(); run(): any { return this.k.sort((x: number, y: number) => x - y); } }
function G_sort(): any { return G.sort((x: number, y: number) => x - y); }
function P_copyWithin(a: number[]): any { return a.copyWithin(0, 1); }
function S_copyWithin(a: number[]): void { a.copyWithin(0, 1); }
class F_copyWithin { k: number[] = mk(); run(): any { return this.k.copyWithin(0, 1); } }
function G_copyWithin(): any { return G.copyWithin(0, 1); }
function P_entries(a: number[]): any { return a.entries(); }
function S_entries(a: number[]): void { a.entries(); }
class F_entries { k: number[] = mk(); run(): any { return this.k.entries(); } }
function G_entries(): any { return G.entries(); }
function P_findLast(a: number[]): any { return a.findLast((x: number) => x > 1); }
function S_findLast(a: number[]): void { a.findLast((x: number) => x > 1); }
class F_findLast { k: number[] = mk(); run(): any { return this.k.findLast((x: number) => x > 1); } }
function G_findLast(): any { return G.findLast((x: number) => x > 1); }
function P_findLastIndex(a: number[]): any { return a.findLastIndex((x: number) => x > 1); }
function S_findLastIndex(a: number[]): void { a.findLastIndex((x: number) => x > 1); }
class F_findLastIndex { k: number[] = mk(); run(): any { return this.k.findLastIndex((x: number) => x > 1); } }
function G_findLastIndex(): any { return G.findLastIndex((x: number) => x > 1); }
function P_flat(a: number[]): any { return a.flat(); }
function S_flat(a: number[]): void { a.flat(); }
class F_flat { k: number[] = mk(); run(): any { return this.k.flat(); } }
function G_flat(): any { return G.flat(); }
function P_flatMap(a: number[]): any { return a.flatMap((x: number) => [x, x]); }
function S_flatMap(a: number[]): void { a.flatMap((x: number) => [x, x]); }
class F_flatMap { k: number[] = mk(); run(): any { return this.k.flatMap((x: number) => [x, x]); } }
function G_flatMap(): any { return G.flatMap((x: number) => [x, x]); }
function P_keys(a: number[]): any { return a.keys(); }
function S_keys(a: number[]): void { a.keys(); }
class F_keys { k: number[] = mk(); run(): any { return this.k.keys(); } }
function G_keys(): any { return G.keys(); }
function P_reduceRight(a: number[]): any { return a.reduceRight((p: number, x: number) => p + x, 0); }
function S_reduceRight(a: number[]): void { a.reduceRight((p: number, x: number) => p + x, 0); }
class F_reduceRight { k: number[] = mk(); run(): any { return this.k.reduceRight((p: number, x: number) => p + x, 0); } }
function G_reduceRight(): any { return G.reduceRight((p: number, x: number) => p + x, 0); }
function P_toReversed(a: number[]): any { return a.toReversed(); }
function S_toReversed(a: number[]): void { a.toReversed(); }
class F_toReversed { k: number[] = mk(); run(): any { return this.k.toReversed(); } }
function G_toReversed(): any { return G.toReversed(); }
function P_toSorted(a: number[]): any { return a.toSorted(); }
function S_toSorted(a: number[]): void { a.toSorted(); }
class F_toSorted { k: number[] = mk(); run(): any { return this.k.toSorted(); } }
function G_toSorted(): any { return G.toSorted(); }
function P_toSpliced(a: number[]): any { return a.toSpliced(0, 1); }
function S_toSpliced(a: number[]): void { a.toSpliced(0, 1); }
class F_toSpliced { k: number[] = mk(); run(): any { return this.k.toSpliced(0, 1); } }
function G_toSpliced(): any { return G.toSpliced(0, 1); }
function P_values(a: number[]): any { return a.values(); }
function S_values(a: number[]): void { a.values(); }
class F_values { k: number[] = mk(); run(): any { return this.k.values(); } }
function G_values(): any { return G.values(); }
function P_with(a: number[]): any { return a.with(0, 9); }
function S_with(a: number[]): void { a.with(0, 9); }
class F_with { k: number[] = mk(); run(): any { return this.k.with(0, 9); } }
function G_with(): any { return G.with(0, 9); }
{ const c: number[] = mk(); T("P pop plain", () => P_pop(c), c); }
{ const c: number[] = mk(); T("S pop plain", () => S_pop(c), c); }
{ const h = new F_pop(); h.k = mk(); T("F pop plain", () => h.run(), h.k); }
{ G = mk(); T("G pop plain", () => G_pop(), G); }
{ const c: number[] = mk(); T("L pop plain", () => c.pop(), c); }
{ const c: number[] = mkOwn(0); T("P pop own", () => P_pop(c), c); }
{ const c: number[] = mkOwn(0); T("S pop own", () => S_pop(c), c); }
{ const h = new F_pop(); h.k = mkOwn(0); T("F pop own", () => h.run(), h.k); }
{ G = mkOwn(0); T("G pop own", () => G_pop(), G); }
{ const c: number[] = mkOwn(0); T("L pop own", () => c.pop(), c); }
{ const c: number[] = mkAcc(0); T("P pop acc", () => P_pop(c), c); }
{ const c: number[] = mkAcc(0); T("S pop acc", () => S_pop(c), c); }
{ const h = new F_pop(); h.k = mkAcc(0); T("F pop acc", () => h.run(), h.k); }
{ G = mkAcc(0); T("G pop acc", () => G_pop(), G); }
{ const c: number[] = mkAcc(0); T("L pop acc", () => c.pop(), c); }
{ const c: number[] = mkProto(0); T("P pop proto", () => P_pop(c), c); }
{ const c: number[] = mkProto(0); T("S pop proto", () => S_pop(c), c); }
{ const h = new F_pop(); h.k = mkProto(0); T("F pop proto", () => h.run(), h.k); }
{ G = mkProto(0); T("G pop proto", () => G_pop(), G); }
{ const c: number[] = mkProto(0); T("L pop proto", () => c.pop(), c); }
{ const c: number[] = mkSub(0); T("P pop sub", () => P_pop(c), c); }
{ const c: number[] = mkSub(0); T("S pop sub", () => S_pop(c), c); }
{ const h = new F_pop(); h.k = mkSub(0); T("F pop sub", () => h.run(), h.k); }
{ G = mkSub(0); T("G pop sub", () => G_pop(), G); }
{ const c: number[] = mkSub(0); T("L pop sub", () => c.pop(), c); }
{ const c: number[] = mkFake(0); T("P pop fake", () => P_pop(c), c); }
{ const c: number[] = mkFake(0); T("S pop fake", () => S_pop(c), c); }
{ const h = new F_pop(); h.k = mkFake(0); T("F pop fake", () => h.run(), h.k); }
{ G = mkFake(0); T("G pop fake", () => G_pop(), G); }
{ const c: number[] = mkFake(0); T("L pop fake", () => c.pop(), c); }
{ const c: number[] = mkNone(0); T("P pop none", () => P_pop(c), c); }
{ const c: number[] = mkNone(0); T("S pop none", () => S_pop(c), c); }
{ const h = new F_pop(); h.k = mkNone(0); T("F pop none", () => h.run(), h.k); }
{ G = mkNone(0); T("G pop none", () => G_pop(), G); }
{ const c: number[] = mkNone(0); T("L pop none", () => c.pop(), c); }
{ const c: number[] = mkNamed(0); T("P pop named", () => P_pop(c), c); }
{ const c: number[] = mkNamed(0); T("S pop named", () => S_pop(c), c); }
{ const h = new F_pop(); h.k = mkNamed(0); T("F pop named", () => h.run(), h.k); }
{ G = mkNamed(0); T("G pop named", () => G_pop(), G); }
{ const c: number[] = mkNamed(0); T("L pop named", () => c.pop(), c); }
{ const c: number[] = mkLent(0); T("P pop lent", () => P_pop(c), c); }
{ const c: number[] = mkLent(0); T("S pop lent", () => S_pop(c), c); }
{ const h = new F_pop(); h.k = mkLent(0); T("F pop lent", () => h.run(), h.k); }
{ G = mkLent(0); T("G pop lent", () => G_pop(), G); }
{ const c: number[] = mkLent(0); T("L pop lent", () => c.pop(), c); }
{ const c: number[] = mk(); T("P pop again", () => P_pop(c), c); }
{ const c: number[] = mk(); T("S pop again", () => S_pop(c), c); }
{ const h = new F_pop(); h.k = mk(); T("F pop again", () => h.run(), h.k); }
{ G = mk(); T("G pop again", () => G_pop(), G); }
{ const c: number[] = mk(); T("L pop again", () => c.pop(), c); }
{ const c: number[] = mk(); T("P shift plain", () => P_shift(c), c); }
{ const c: number[] = mk(); T("S shift plain", () => S_shift(c), c); }
{ const h = new F_shift(); h.k = mk(); T("F shift plain", () => h.run(), h.k); }
{ G = mk(); T("G shift plain", () => G_shift(), G); }
{ const c: number[] = mk(); T("L shift plain", () => c.shift(), c); }
{ const c: number[] = mkOwn(1); T("P shift own", () => P_shift(c), c); }
{ const c: number[] = mkOwn(1); T("S shift own", () => S_shift(c), c); }
{ const h = new F_shift(); h.k = mkOwn(1); T("F shift own", () => h.run(), h.k); }
{ G = mkOwn(1); T("G shift own", () => G_shift(), G); }
{ const c: number[] = mkOwn(1); T("L shift own", () => c.shift(), c); }
{ const c: number[] = mkAcc(1); T("P shift acc", () => P_shift(c), c); }
{ const c: number[] = mkAcc(1); T("S shift acc", () => S_shift(c), c); }
{ const h = new F_shift(); h.k = mkAcc(1); T("F shift acc", () => h.run(), h.k); }
{ G = mkAcc(1); T("G shift acc", () => G_shift(), G); }
{ const c: number[] = mkAcc(1); T("L shift acc", () => c.shift(), c); }
{ const c: number[] = mkProto(1); T("P shift proto", () => P_shift(c), c); }
{ const c: number[] = mkProto(1); T("S shift proto", () => S_shift(c), c); }
{ const h = new F_shift(); h.k = mkProto(1); T("F shift proto", () => h.run(), h.k); }
{ G = mkProto(1); T("G shift proto", () => G_shift(), G); }
{ const c: number[] = mkProto(1); T("L shift proto", () => c.shift(), c); }
{ const c: number[] = mkSub(1); T("P shift sub", () => P_shift(c), c); }
{ const c: number[] = mkSub(1); T("S shift sub", () => S_shift(c), c); }
{ const h = new F_shift(); h.k = mkSub(1); T("F shift sub", () => h.run(), h.k); }
{ G = mkSub(1); T("G shift sub", () => G_shift(), G); }
{ const c: number[] = mkSub(1); T("L shift sub", () => c.shift(), c); }
{ const c: number[] = mkFake(1); T("P shift fake", () => P_shift(c), c); }
{ const c: number[] = mkFake(1); T("S shift fake", () => S_shift(c), c); }
{ const h = new F_shift(); h.k = mkFake(1); T("F shift fake", () => h.run(), h.k); }
{ G = mkFake(1); T("G shift fake", () => G_shift(), G); }
{ const c: number[] = mkFake(1); T("L shift fake", () => c.shift(), c); }
{ const c: number[] = mkNone(1); T("P shift none", () => P_shift(c), c); }
{ const c: number[] = mkNone(1); T("S shift none", () => S_shift(c), c); }
{ const h = new F_shift(); h.k = mkNone(1); T("F shift none", () => h.run(), h.k); }
{ G = mkNone(1); T("G shift none", () => G_shift(), G); }
{ const c: number[] = mkNone(1); T("L shift none", () => c.shift(), c); }
{ const c: number[] = mkNamed(1); T("P shift named", () => P_shift(c), c); }
{ const c: number[] = mkNamed(1); T("S shift named", () => S_shift(c), c); }
{ const h = new F_shift(); h.k = mkNamed(1); T("F shift named", () => h.run(), h.k); }
{ G = mkNamed(1); T("G shift named", () => G_shift(), G); }
{ const c: number[] = mkNamed(1); T("L shift named", () => c.shift(), c); }
{ const c: number[] = mkLent(1); T("P shift lent", () => P_shift(c), c); }
{ const c: number[] = mkLent(1); T("S shift lent", () => S_shift(c), c); }
{ const h = new F_shift(); h.k = mkLent(1); T("F shift lent", () => h.run(), h.k); }
{ G = mkLent(1); T("G shift lent", () => G_shift(), G); }
{ const c: number[] = mkLent(1); T("L shift lent", () => c.shift(), c); }
{ const c: number[] = mk(); T("P shift again", () => P_shift(c), c); }
{ const c: number[] = mk(); T("S shift again", () => S_shift(c), c); }
{ const h = new F_shift(); h.k = mk(); T("F shift again", () => h.run(), h.k); }
{ G = mk(); T("G shift again", () => G_shift(), G); }
{ const c: number[] = mk(); T("L shift again", () => c.shift(), c); }
{ const c: number[] = mk(); T("P unshift plain", () => P_unshift(c), c); }
{ const c: number[] = mk(); T("S unshift plain", () => S_unshift(c), c); }
{ const h = new F_unshift(); h.k = mk(); T("F unshift plain", () => h.run(), h.k); }
{ G = mk(); T("G unshift plain", () => G_unshift(), G); }
{ const c: number[] = mk(); T("L unshift plain", () => c.unshift(7), c); }
{ const c: number[] = mkOwn(2); T("P unshift own", () => P_unshift(c), c); }
{ const c: number[] = mkOwn(2); T("S unshift own", () => S_unshift(c), c); }
{ const h = new F_unshift(); h.k = mkOwn(2); T("F unshift own", () => h.run(), h.k); }
{ G = mkOwn(2); T("G unshift own", () => G_unshift(), G); }
{ const c: number[] = mkOwn(2); T("L unshift own", () => c.unshift(7), c); }
{ const c: number[] = mkAcc(2); T("P unshift acc", () => P_unshift(c), c); }
{ const c: number[] = mkAcc(2); T("S unshift acc", () => S_unshift(c), c); }
{ const h = new F_unshift(); h.k = mkAcc(2); T("F unshift acc", () => h.run(), h.k); }
{ G = mkAcc(2); T("G unshift acc", () => G_unshift(), G); }
{ const c: number[] = mkAcc(2); T("L unshift acc", () => c.unshift(7), c); }
{ const c: number[] = mkProto(2); T("P unshift proto", () => P_unshift(c), c); }
{ const c: number[] = mkProto(2); T("S unshift proto", () => S_unshift(c), c); }
{ const h = new F_unshift(); h.k = mkProto(2); T("F unshift proto", () => h.run(), h.k); }
{ G = mkProto(2); T("G unshift proto", () => G_unshift(), G); }
{ const c: number[] = mkProto(2); T("L unshift proto", () => c.unshift(7), c); }
{ const c: number[] = mkSub(2); T("P unshift sub", () => P_unshift(c), c); }
{ const c: number[] = mkSub(2); T("S unshift sub", () => S_unshift(c), c); }
{ const h = new F_unshift(); h.k = mkSub(2); T("F unshift sub", () => h.run(), h.k); }
{ G = mkSub(2); T("G unshift sub", () => G_unshift(), G); }
{ const c: number[] = mkSub(2); T("L unshift sub", () => c.unshift(7), c); }
{ const c: number[] = mkFake(2); T("P unshift fake", () => P_unshift(c), c); }
{ const c: number[] = mkFake(2); T("S unshift fake", () => S_unshift(c), c); }
{ const h = new F_unshift(); h.k = mkFake(2); T("F unshift fake", () => h.run(), h.k); }
{ G = mkFake(2); T("G unshift fake", () => G_unshift(), G); }
{ const c: number[] = mkFake(2); T("L unshift fake", () => c.unshift(7), c); }
{ const c: number[] = mkNone(2); T("P unshift none", () => P_unshift(c), c); }
{ const c: number[] = mkNone(2); T("S unshift none", () => S_unshift(c), c); }
{ const h = new F_unshift(); h.k = mkNone(2); T("F unshift none", () => h.run(), h.k); }
{ G = mkNone(2); T("G unshift none", () => G_unshift(), G); }
{ const c: number[] = mkNone(2); T("L unshift none", () => c.unshift(7), c); }
{ const c: number[] = mkNamed(2); T("P unshift named", () => P_unshift(c), c); }
{ const c: number[] = mkNamed(2); T("S unshift named", () => S_unshift(c), c); }
{ const h = new F_unshift(); h.k = mkNamed(2); T("F unshift named", () => h.run(), h.k); }
{ G = mkNamed(2); T("G unshift named", () => G_unshift(), G); }
{ const c: number[] = mkNamed(2); T("L unshift named", () => c.unshift(7), c); }
{ const c: number[] = mkLent(2); T("P unshift lent", () => P_unshift(c), c); }
{ const c: number[] = mkLent(2); T("S unshift lent", () => S_unshift(c), c); }
{ const h = new F_unshift(); h.k = mkLent(2); T("F unshift lent", () => h.run(), h.k); }
{ G = mkLent(2); T("G unshift lent", () => G_unshift(), G); }
{ const c: number[] = mkLent(2); T("L unshift lent", () => c.unshift(7), c); }
{ const c: number[] = mk(); T("P unshift again", () => P_unshift(c), c); }
{ const c: number[] = mk(); T("S unshift again", () => S_unshift(c), c); }
{ const h = new F_unshift(); h.k = mk(); T("F unshift again", () => h.run(), h.k); }
{ G = mk(); T("G unshift again", () => G_unshift(), G); }
{ const c: number[] = mk(); T("L unshift again", () => c.unshift(7), c); }
{ const c: number[] = mk(); T("P indexOf plain", () => P_indexOf(c), c); }
{ const c: number[] = mk(); T("S indexOf plain", () => S_indexOf(c), c); }
{ const h = new F_indexOf(); h.k = mk(); T("F indexOf plain", () => h.run(), h.k); }
{ G = mk(); T("G indexOf plain", () => G_indexOf(), G); }
{ const c: number[] = mk(); T("L indexOf plain", () => c.indexOf(1), c); }
{ const c: number[] = mkOwn(3); T("P indexOf own", () => P_indexOf(c), c); }
{ const c: number[] = mkOwn(3); T("S indexOf own", () => S_indexOf(c), c); }
{ const h = new F_indexOf(); h.k = mkOwn(3); T("F indexOf own", () => h.run(), h.k); }
{ G = mkOwn(3); T("G indexOf own", () => G_indexOf(), G); }
{ const c: number[] = mkOwn(3); T("L indexOf own", () => c.indexOf(1), c); }
{ const c: number[] = mkAcc(3); T("P indexOf acc", () => P_indexOf(c), c); }
{ const c: number[] = mkAcc(3); T("S indexOf acc", () => S_indexOf(c), c); }
{ const h = new F_indexOf(); h.k = mkAcc(3); T("F indexOf acc", () => h.run(), h.k); }
{ G = mkAcc(3); T("G indexOf acc", () => G_indexOf(), G); }
{ const c: number[] = mkAcc(3); T("L indexOf acc", () => c.indexOf(1), c); }
{ const c: number[] = mkProto(3); T("P indexOf proto", () => P_indexOf(c), c); }
{ const c: number[] = mkProto(3); T("S indexOf proto", () => S_indexOf(c), c); }
{ const h = new F_indexOf(); h.k = mkProto(3); T("F indexOf proto", () => h.run(), h.k); }
{ G = mkProto(3); T("G indexOf proto", () => G_indexOf(), G); }
{ const c: number[] = mkProto(3); T("L indexOf proto", () => c.indexOf(1), c); }
{ const c: number[] = mkSub(3); T("P indexOf sub", () => P_indexOf(c), c); }
{ const c: number[] = mkSub(3); T("S indexOf sub", () => S_indexOf(c), c); }
{ const h = new F_indexOf(); h.k = mkSub(3); T("F indexOf sub", () => h.run(), h.k); }
{ G = mkSub(3); T("G indexOf sub", () => G_indexOf(), G); }
{ const c: number[] = mkSub(3); T("L indexOf sub", () => c.indexOf(1), c); }
{ const c: number[] = mkFake(3); T("P indexOf fake", () => P_indexOf(c), c); }
{ const c: number[] = mkFake(3); T("S indexOf fake", () => S_indexOf(c), c); }
{ const h = new F_indexOf(); h.k = mkFake(3); T("F indexOf fake", () => h.run(), h.k); }
{ G = mkFake(3); T("G indexOf fake", () => G_indexOf(), G); }
{ const c: number[] = mkFake(3); T("L indexOf fake", () => c.indexOf(1), c); }
{ const c: number[] = mkNone(3); T("P indexOf none", () => P_indexOf(c), c); }
{ const c: number[] = mkNone(3); T("S indexOf none", () => S_indexOf(c), c); }
{ const h = new F_indexOf(); h.k = mkNone(3); T("F indexOf none", () => h.run(), h.k); }
{ G = mkNone(3); T("G indexOf none", () => G_indexOf(), G); }
{ const c: number[] = mkNone(3); T("L indexOf none", () => c.indexOf(1), c); }
{ const c: number[] = mkNamed(3); T("P indexOf named", () => P_indexOf(c), c); }
{ const c: number[] = mkNamed(3); T("S indexOf named", () => S_indexOf(c), c); }
{ const h = new F_indexOf(); h.k = mkNamed(3); T("F indexOf named", () => h.run(), h.k); }
{ G = mkNamed(3); T("G indexOf named", () => G_indexOf(), G); }
{ const c: number[] = mkNamed(3); T("L indexOf named", () => c.indexOf(1), c); }
{ const c: number[] = mkLent(3); T("P indexOf lent", () => P_indexOf(c), c); }
{ const c: number[] = mkLent(3); T("S indexOf lent", () => S_indexOf(c), c); }
{ const h = new F_indexOf(); h.k = mkLent(3); T("F indexOf lent", () => h.run(), h.k); }
{ G = mkLent(3); T("G indexOf lent", () => G_indexOf(), G); }
{ const c: number[] = mkLent(3); T("L indexOf lent", () => c.indexOf(1), c); }
{ const c: number[] = mk(); T("P indexOf again", () => P_indexOf(c), c); }
{ const c: number[] = mk(); T("S indexOf again", () => S_indexOf(c), c); }
{ const h = new F_indexOf(); h.k = mk(); T("F indexOf again", () => h.run(), h.k); }
{ G = mk(); T("G indexOf again", () => G_indexOf(), G); }
{ const c: number[] = mk(); T("L indexOf again", () => c.indexOf(1), c); }
{ const c: number[] = mk(); T("P lastIndexOf plain", () => P_lastIndexOf(c), c); }
{ const c: number[] = mk(); T("S lastIndexOf plain", () => S_lastIndexOf(c), c); }
{ const h = new F_lastIndexOf(); h.k = mk(); T("F lastIndexOf plain", () => h.run(), h.k); }
{ G = mk(); T("G lastIndexOf plain", () => G_lastIndexOf(), G); }
{ const c: number[] = mk(); T("L lastIndexOf plain", () => c.lastIndexOf(1), c); }
{ const c: number[] = mkOwn(4); T("P lastIndexOf own", () => P_lastIndexOf(c), c); }
{ const c: number[] = mkOwn(4); T("S lastIndexOf own", () => S_lastIndexOf(c), c); }
{ const h = new F_lastIndexOf(); h.k = mkOwn(4); T("F lastIndexOf own", () => h.run(), h.k); }
{ G = mkOwn(4); T("G lastIndexOf own", () => G_lastIndexOf(), G); }
{ const c: number[] = mkOwn(4); T("L lastIndexOf own", () => c.lastIndexOf(1), c); }
{ const c: number[] = mkAcc(4); T("P lastIndexOf acc", () => P_lastIndexOf(c), c); }
{ const c: number[] = mkAcc(4); T("S lastIndexOf acc", () => S_lastIndexOf(c), c); }
{ const h = new F_lastIndexOf(); h.k = mkAcc(4); T("F lastIndexOf acc", () => h.run(), h.k); }
{ G = mkAcc(4); T("G lastIndexOf acc", () => G_lastIndexOf(), G); }
{ const c: number[] = mkAcc(4); T("L lastIndexOf acc", () => c.lastIndexOf(1), c); }
{ const c: number[] = mkProto(4); T("P lastIndexOf proto", () => P_lastIndexOf(c), c); }
{ const c: number[] = mkProto(4); T("S lastIndexOf proto", () => S_lastIndexOf(c), c); }
{ const h = new F_lastIndexOf(); h.k = mkProto(4); T("F lastIndexOf proto", () => h.run(), h.k); }
{ G = mkProto(4); T("G lastIndexOf proto", () => G_lastIndexOf(), G); }
{ const c: number[] = mkProto(4); T("L lastIndexOf proto", () => c.lastIndexOf(1), c); }
{ const c: number[] = mkSub(4); T("P lastIndexOf sub", () => P_lastIndexOf(c), c); }
{ const c: number[] = mkSub(4); T("S lastIndexOf sub", () => S_lastIndexOf(c), c); }
{ const h = new F_lastIndexOf(); h.k = mkSub(4); T("F lastIndexOf sub", () => h.run(), h.k); }
{ G = mkSub(4); T("G lastIndexOf sub", () => G_lastIndexOf(), G); }
{ const c: number[] = mkSub(4); T("L lastIndexOf sub", () => c.lastIndexOf(1), c); }
{ const c: number[] = mkFake(4); T("P lastIndexOf fake", () => P_lastIndexOf(c), c); }
{ const c: number[] = mkFake(4); T("S lastIndexOf fake", () => S_lastIndexOf(c), c); }
{ const h = new F_lastIndexOf(); h.k = mkFake(4); T("F lastIndexOf fake", () => h.run(), h.k); }
{ G = mkFake(4); T("G lastIndexOf fake", () => G_lastIndexOf(), G); }
{ const c: number[] = mkFake(4); T("L lastIndexOf fake", () => c.lastIndexOf(1), c); }
{ const c: number[] = mkNone(4); T("P lastIndexOf none", () => P_lastIndexOf(c), c); }
{ const c: number[] = mkNone(4); T("S lastIndexOf none", () => S_lastIndexOf(c), c); }
{ const h = new F_lastIndexOf(); h.k = mkNone(4); T("F lastIndexOf none", () => h.run(), h.k); }
{ G = mkNone(4); T("G lastIndexOf none", () => G_lastIndexOf(), G); }
{ const c: number[] = mkNone(4); T("L lastIndexOf none", () => c.lastIndexOf(1), c); }
{ const c: number[] = mkNamed(4); T("P lastIndexOf named", () => P_lastIndexOf(c), c); }
{ const c: number[] = mkNamed(4); T("S lastIndexOf named", () => S_lastIndexOf(c), c); }
{ const h = new F_lastIndexOf(); h.k = mkNamed(4); T("F lastIndexOf named", () => h.run(), h.k); }
{ G = mkNamed(4); T("G lastIndexOf named", () => G_lastIndexOf(), G); }
{ const c: number[] = mkNamed(4); T("L lastIndexOf named", () => c.lastIndexOf(1), c); }
{ const c: number[] = mkLent(4); T("P lastIndexOf lent", () => P_lastIndexOf(c), c); }
{ const c: number[] = mkLent(4); T("S lastIndexOf lent", () => S_lastIndexOf(c), c); }
{ const h = new F_lastIndexOf(); h.k = mkLent(4); T("F lastIndexOf lent", () => h.run(), h.k); }
{ G = mkLent(4); T("G lastIndexOf lent", () => G_lastIndexOf(), G); }
{ const c: number[] = mkLent(4); T("L lastIndexOf lent", () => c.lastIndexOf(1), c); }
{ const c: number[] = mk(); T("P lastIndexOf again", () => P_lastIndexOf(c), c); }
{ const c: number[] = mk(); T("S lastIndexOf again", () => S_lastIndexOf(c), c); }
{ const h = new F_lastIndexOf(); h.k = mk(); T("F lastIndexOf again", () => h.run(), h.k); }
{ G = mk(); T("G lastIndexOf again", () => G_lastIndexOf(), G); }
{ const c: number[] = mk(); T("L lastIndexOf again", () => c.lastIndexOf(1), c); }
{ const c: number[] = mk(); T("P includes plain", () => P_includes(c), c); }
{ const c: number[] = mk(); T("S includes plain", () => S_includes(c), c); }
{ const h = new F_includes(); h.k = mk(); T("F includes plain", () => h.run(), h.k); }
{ G = mk(); T("G includes plain", () => G_includes(), G); }
{ const c: number[] = mk(); T("L includes plain", () => c.includes(1), c); }
{ const c: number[] = mkOwn(5); T("P includes own", () => P_includes(c), c); }
{ const c: number[] = mkOwn(5); T("S includes own", () => S_includes(c), c); }
{ const h = new F_includes(); h.k = mkOwn(5); T("F includes own", () => h.run(), h.k); }
{ G = mkOwn(5); T("G includes own", () => G_includes(), G); }
{ const c: number[] = mkOwn(5); T("L includes own", () => c.includes(1), c); }
{ const c: number[] = mkAcc(5); T("P includes acc", () => P_includes(c), c); }
{ const c: number[] = mkAcc(5); T("S includes acc", () => S_includes(c), c); }
{ const h = new F_includes(); h.k = mkAcc(5); T("F includes acc", () => h.run(), h.k); }
{ G = mkAcc(5); T("G includes acc", () => G_includes(), G); }
{ const c: number[] = mkAcc(5); T("L includes acc", () => c.includes(1), c); }
{ const c: number[] = mkProto(5); T("P includes proto", () => P_includes(c), c); }
{ const c: number[] = mkProto(5); T("S includes proto", () => S_includes(c), c); }
{ const h = new F_includes(); h.k = mkProto(5); T("F includes proto", () => h.run(), h.k); }
{ G = mkProto(5); T("G includes proto", () => G_includes(), G); }
{ const c: number[] = mkProto(5); T("L includes proto", () => c.includes(1), c); }
{ const c: number[] = mkSub(5); T("P includes sub", () => P_includes(c), c); }
{ const c: number[] = mkSub(5); T("S includes sub", () => S_includes(c), c); }
{ const h = new F_includes(); h.k = mkSub(5); T("F includes sub", () => h.run(), h.k); }
{ G = mkSub(5); T("G includes sub", () => G_includes(), G); }
{ const c: number[] = mkSub(5); T("L includes sub", () => c.includes(1), c); }
{ const c: number[] = mkFake(5); T("P includes fake", () => P_includes(c), c); }
{ const c: number[] = mkFake(5); T("S includes fake", () => S_includes(c), c); }
{ const h = new F_includes(); h.k = mkFake(5); T("F includes fake", () => h.run(), h.k); }
{ G = mkFake(5); T("G includes fake", () => G_includes(), G); }
{ const c: number[] = mkFake(5); T("L includes fake", () => c.includes(1), c); }
{ const c: number[] = mkNone(5); T("P includes none", () => P_includes(c), c); }
{ const c: number[] = mkNone(5); T("S includes none", () => S_includes(c), c); }
{ const h = new F_includes(); h.k = mkNone(5); T("F includes none", () => h.run(), h.k); }
{ G = mkNone(5); T("G includes none", () => G_includes(), G); }
{ const c: number[] = mkNone(5); T("L includes none", () => c.includes(1), c); }
{ const c: number[] = mkNamed(5); T("P includes named", () => P_includes(c), c); }
{ const c: number[] = mkNamed(5); T("S includes named", () => S_includes(c), c); }
{ const h = new F_includes(); h.k = mkNamed(5); T("F includes named", () => h.run(), h.k); }
{ G = mkNamed(5); T("G includes named", () => G_includes(), G); }
{ const c: number[] = mkNamed(5); T("L includes named", () => c.includes(1), c); }
{ const c: number[] = mkLent(5); T("P includes lent", () => P_includes(c), c); }
{ const c: number[] = mkLent(5); T("S includes lent", () => S_includes(c), c); }
{ const h = new F_includes(); h.k = mkLent(5); T("F includes lent", () => h.run(), h.k); }
{ G = mkLent(5); T("G includes lent", () => G_includes(), G); }
{ const c: number[] = mkLent(5); T("L includes lent", () => c.includes(1), c); }
{ const c: number[] = mk(); T("P includes again", () => P_includes(c), c); }
{ const c: number[] = mk(); T("S includes again", () => S_includes(c), c); }
{ const h = new F_includes(); h.k = mk(); T("F includes again", () => h.run(), h.k); }
{ G = mk(); T("G includes again", () => G_includes(), G); }
{ const c: number[] = mk(); T("L includes again", () => c.includes(1), c); }
{ const c: number[] = mk(); T("P slice plain", () => P_slice(c), c); }
{ const c: number[] = mk(); T("S slice plain", () => S_slice(c), c); }
{ const h = new F_slice(); h.k = mk(); T("F slice plain", () => h.run(), h.k); }
{ G = mk(); T("G slice plain", () => G_slice(), G); }
{ const c: number[] = mk(); T("L slice plain", () => c.slice(0, 2), c); }
{ const c: number[] = mkOwn(6); T("P slice own", () => P_slice(c), c); }
{ const c: number[] = mkOwn(6); T("S slice own", () => S_slice(c), c); }
{ const h = new F_slice(); h.k = mkOwn(6); T("F slice own", () => h.run(), h.k); }
{ G = mkOwn(6); T("G slice own", () => G_slice(), G); }
{ const c: number[] = mkOwn(6); T("L slice own", () => c.slice(0, 2), c); }
{ const c: number[] = mkAcc(6); T("P slice acc", () => P_slice(c), c); }
{ const c: number[] = mkAcc(6); T("S slice acc", () => S_slice(c), c); }
{ const h = new F_slice(); h.k = mkAcc(6); T("F slice acc", () => h.run(), h.k); }
{ G = mkAcc(6); T("G slice acc", () => G_slice(), G); }
{ const c: number[] = mkAcc(6); T("L slice acc", () => c.slice(0, 2), c); }
{ const c: number[] = mkProto(6); T("P slice proto", () => P_slice(c), c); }
{ const c: number[] = mkProto(6); T("S slice proto", () => S_slice(c), c); }
{ const h = new F_slice(); h.k = mkProto(6); T("F slice proto", () => h.run(), h.k); }
{ G = mkProto(6); T("G slice proto", () => G_slice(), G); }
{ const c: number[] = mkProto(6); T("L slice proto", () => c.slice(0, 2), c); }
{ const c: number[] = mkSub(6); T("P slice sub", () => P_slice(c), c); }
{ const c: number[] = mkSub(6); T("S slice sub", () => S_slice(c), c); }
{ const h = new F_slice(); h.k = mkSub(6); T("F slice sub", () => h.run(), h.k); }
{ G = mkSub(6); T("G slice sub", () => G_slice(), G); }
{ const c: number[] = mkSub(6); T("L slice sub", () => c.slice(0, 2), c); }
{ const c: number[] = mkFake(6); T("P slice fake", () => P_slice(c), c); }
{ const c: number[] = mkFake(6); T("S slice fake", () => S_slice(c), c); }
{ const h = new F_slice(); h.k = mkFake(6); T("F slice fake", () => h.run(), h.k); }
{ G = mkFake(6); T("G slice fake", () => G_slice(), G); }
{ const c: number[] = mkFake(6); T("L slice fake", () => c.slice(0, 2), c); }
{ const c: number[] = mkNone(6); T("P slice none", () => P_slice(c), c); }
{ const c: number[] = mkNone(6); T("S slice none", () => S_slice(c), c); }
{ const h = new F_slice(); h.k = mkNone(6); T("F slice none", () => h.run(), h.k); }
{ G = mkNone(6); T("G slice none", () => G_slice(), G); }
{ const c: number[] = mkNone(6); T("L slice none", () => c.slice(0, 2), c); }
{ const c: number[] = mkNamed(6); T("P slice named", () => P_slice(c), c); }
{ const c: number[] = mkNamed(6); T("S slice named", () => S_slice(c), c); }
{ const h = new F_slice(); h.k = mkNamed(6); T("F slice named", () => h.run(), h.k); }
{ G = mkNamed(6); T("G slice named", () => G_slice(), G); }
{ const c: number[] = mkNamed(6); T("L slice named", () => c.slice(0, 2), c); }
{ const c: number[] = mkLent(6); T("P slice lent", () => P_slice(c), c); }
{ const c: number[] = mkLent(6); T("S slice lent", () => S_slice(c), c); }
{ const h = new F_slice(); h.k = mkLent(6); T("F slice lent", () => h.run(), h.k); }
{ G = mkLent(6); T("G slice lent", () => G_slice(), G); }
{ const c: number[] = mkLent(6); T("L slice lent", () => c.slice(0, 2), c); }
{ const c: number[] = mk(); T("P slice again", () => P_slice(c), c); }
{ const c: number[] = mk(); T("S slice again", () => S_slice(c), c); }
{ const h = new F_slice(); h.k = mk(); T("F slice again", () => h.run(), h.k); }
{ G = mk(); T("G slice again", () => G_slice(), G); }
{ const c: number[] = mk(); T("L slice again", () => c.slice(0, 2), c); }
{ const c: number[] = mk(); T("P splice plain", () => P_splice(c), c); }
{ const c: number[] = mk(); T("S splice plain", () => S_splice(c), c); }
{ const h = new F_splice(); h.k = mk(); T("F splice plain", () => h.run(), h.k); }
{ G = mk(); T("G splice plain", () => G_splice(), G); }
{ const c: number[] = mk(); T("L splice plain", () => c.splice(0, 1), c); }
{ const c: number[] = mkOwn(7); T("P splice own", () => P_splice(c), c); }
{ const c: number[] = mkOwn(7); T("S splice own", () => S_splice(c), c); }
{ const h = new F_splice(); h.k = mkOwn(7); T("F splice own", () => h.run(), h.k); }
{ G = mkOwn(7); T("G splice own", () => G_splice(), G); }
{ const c: number[] = mkOwn(7); T("L splice own", () => c.splice(0, 1), c); }
{ const c: number[] = mkAcc(7); T("P splice acc", () => P_splice(c), c); }
{ const c: number[] = mkAcc(7); T("S splice acc", () => S_splice(c), c); }
{ const h = new F_splice(); h.k = mkAcc(7); T("F splice acc", () => h.run(), h.k); }
{ G = mkAcc(7); T("G splice acc", () => G_splice(), G); }
{ const c: number[] = mkAcc(7); T("L splice acc", () => c.splice(0, 1), c); }
{ const c: number[] = mkProto(7); T("P splice proto", () => P_splice(c), c); }
{ const c: number[] = mkProto(7); T("S splice proto", () => S_splice(c), c); }
{ const h = new F_splice(); h.k = mkProto(7); T("F splice proto", () => h.run(), h.k); }
{ G = mkProto(7); T("G splice proto", () => G_splice(), G); }
{ const c: number[] = mkProto(7); T("L splice proto", () => c.splice(0, 1), c); }
{ const c: number[] = mkSub(7); T("P splice sub", () => P_splice(c), c); }
{ const c: number[] = mkSub(7); T("S splice sub", () => S_splice(c), c); }
{ const h = new F_splice(); h.k = mkSub(7); T("F splice sub", () => h.run(), h.k); }
{ G = mkSub(7); T("G splice sub", () => G_splice(), G); }
{ const c: number[] = mkSub(7); T("L splice sub", () => c.splice(0, 1), c); }
{ const c: number[] = mkFake(7); T("P splice fake", () => P_splice(c), c); }
{ const c: number[] = mkFake(7); T("S splice fake", () => S_splice(c), c); }
{ const h = new F_splice(); h.k = mkFake(7); T("F splice fake", () => h.run(), h.k); }
{ G = mkFake(7); T("G splice fake", () => G_splice(), G); }
{ const c: number[] = mkFake(7); T("L splice fake", () => c.splice(0, 1), c); }
{ const c: number[] = mkNone(7); T("P splice none", () => P_splice(c), c); }
{ const c: number[] = mkNone(7); T("S splice none", () => S_splice(c), c); }
{ const h = new F_splice(); h.k = mkNone(7); T("F splice none", () => h.run(), h.k); }
{ G = mkNone(7); T("G splice none", () => G_splice(), G); }
{ const c: number[] = mkNone(7); T("L splice none", () => c.splice(0, 1), c); }
{ const c: number[] = mkNamed(7); T("P splice named", () => P_splice(c), c); }
{ const c: number[] = mkNamed(7); T("S splice named", () => S_splice(c), c); }
{ const h = new F_splice(); h.k = mkNamed(7); T("F splice named", () => h.run(), h.k); }
{ G = mkNamed(7); T("G splice named", () => G_splice(), G); }
{ const c: number[] = mkNamed(7); T("L splice named", () => c.splice(0, 1), c); }
{ const c: number[] = mkLent(7); T("P splice lent", () => P_splice(c), c); }
{ const c: number[] = mkLent(7); T("S splice lent", () => S_splice(c), c); }
{ const h = new F_splice(); h.k = mkLent(7); T("F splice lent", () => h.run(), h.k); }
{ G = mkLent(7); T("G splice lent", () => G_splice(), G); }
{ const c: number[] = mkLent(7); T("L splice lent", () => c.splice(0, 1), c); }
{ const c: number[] = mk(); T("P splice again", () => P_splice(c), c); }
{ const c: number[] = mk(); T("S splice again", () => S_splice(c), c); }
{ const h = new F_splice(); h.k = mk(); T("F splice again", () => h.run(), h.k); }
{ G = mk(); T("G splice again", () => G_splice(), G); }
{ const c: number[] = mk(); T("L splice again", () => c.splice(0, 1), c); }
{ const c: number[] = mk(); T("P concat plain", () => P_concat(c), c); }
{ const c: number[] = mk(); T("S concat plain", () => S_concat(c), c); }
{ const h = new F_concat(); h.k = mk(); T("F concat plain", () => h.run(), h.k); }
{ G = mk(); T("G concat plain", () => G_concat(), G); }
{ const c: number[] = mk(); T("L concat plain", () => c.concat([9]), c); }
{ const c: number[] = mkOwn(8); T("P concat own", () => P_concat(c), c); }
{ const c: number[] = mkOwn(8); T("S concat own", () => S_concat(c), c); }
{ const h = new F_concat(); h.k = mkOwn(8); T("F concat own", () => h.run(), h.k); }
{ G = mkOwn(8); T("G concat own", () => G_concat(), G); }
{ const c: number[] = mkOwn(8); T("L concat own", () => c.concat([9]), c); }
{ const c: number[] = mkAcc(8); T("P concat acc", () => P_concat(c), c); }
{ const c: number[] = mkAcc(8); T("S concat acc", () => S_concat(c), c); }
{ const h = new F_concat(); h.k = mkAcc(8); T("F concat acc", () => h.run(), h.k); }
{ G = mkAcc(8); T("G concat acc", () => G_concat(), G); }
{ const c: number[] = mkAcc(8); T("L concat acc", () => c.concat([9]), c); }
{ const c: number[] = mkProto(8); T("P concat proto", () => P_concat(c), c); }
{ const c: number[] = mkProto(8); T("S concat proto", () => S_concat(c), c); }
{ const h = new F_concat(); h.k = mkProto(8); T("F concat proto", () => h.run(), h.k); }
{ G = mkProto(8); T("G concat proto", () => G_concat(), G); }
{ const c: number[] = mkProto(8); T("L concat proto", () => c.concat([9]), c); }
{ const c: number[] = mkSub(8); T("P concat sub", () => P_concat(c), c); }
{ const c: number[] = mkSub(8); T("S concat sub", () => S_concat(c), c); }
{ const h = new F_concat(); h.k = mkSub(8); T("F concat sub", () => h.run(), h.k); }
{ G = mkSub(8); T("G concat sub", () => G_concat(), G); }
{ const c: number[] = mkSub(8); T("L concat sub", () => c.concat([9]), c); }
{ const c: number[] = mkFake(8); T("P concat fake", () => P_concat(c), c); }
{ const c: number[] = mkFake(8); T("S concat fake", () => S_concat(c), c); }
{ const h = new F_concat(); h.k = mkFake(8); T("F concat fake", () => h.run(), h.k); }
{ G = mkFake(8); T("G concat fake", () => G_concat(), G); }
{ const c: number[] = mkFake(8); T("L concat fake", () => c.concat([9]), c); }
{ const c: number[] = mkNone(8); T("P concat none", () => P_concat(c), c); }
{ const c: number[] = mkNone(8); T("S concat none", () => S_concat(c), c); }
{ const h = new F_concat(); h.k = mkNone(8); T("F concat none", () => h.run(), h.k); }
{ G = mkNone(8); T("G concat none", () => G_concat(), G); }
{ const c: number[] = mkNone(8); T("L concat none", () => c.concat([9]), c); }
{ const c: number[] = mkNamed(8); T("P concat named", () => P_concat(c), c); }
{ const c: number[] = mkNamed(8); T("S concat named", () => S_concat(c), c); }
{ const h = new F_concat(); h.k = mkNamed(8); T("F concat named", () => h.run(), h.k); }
{ G = mkNamed(8); T("G concat named", () => G_concat(), G); }
{ const c: number[] = mkNamed(8); T("L concat named", () => c.concat([9]), c); }
{ const c: number[] = mkLent(8); T("P concat lent", () => P_concat(c), c); }
{ const c: number[] = mkLent(8); T("S concat lent", () => S_concat(c), c); }
{ const h = new F_concat(); h.k = mkLent(8); T("F concat lent", () => h.run(), h.k); }
{ G = mkLent(8); T("G concat lent", () => G_concat(), G); }
{ const c: number[] = mkLent(8); T("L concat lent", () => c.concat([9]), c); }
{ const c: number[] = mk(); T("P concat again", () => P_concat(c), c); }
{ const c: number[] = mk(); T("S concat again", () => S_concat(c), c); }
{ const h = new F_concat(); h.k = mk(); T("F concat again", () => h.run(), h.k); }
{ G = mk(); T("G concat again", () => G_concat(), G); }
{ const c: number[] = mk(); T("L concat again", () => c.concat([9]), c); }
{ const c: number[] = mk(); T("P join plain", () => P_join(c), c); }
{ const c: number[] = mk(); T("S join plain", () => S_join(c), c); }
{ const h = new F_join(); h.k = mk(); T("F join plain", () => h.run(), h.k); }
{ G = mk(); T("G join plain", () => G_join(), G); }
{ const c: number[] = mk(); T("L join plain", () => c.join("-"), c); }
{ const c: number[] = mkOwn(9); T("P join own", () => P_join(c), c); }
{ const c: number[] = mkOwn(9); T("S join own", () => S_join(c), c); }
{ const h = new F_join(); h.k = mkOwn(9); T("F join own", () => h.run(), h.k); }
{ G = mkOwn(9); T("G join own", () => G_join(), G); }
{ const c: number[] = mkOwn(9); T("L join own", () => c.join("-"), c); }
{ const c: number[] = mkAcc(9); T("P join acc", () => P_join(c), c); }
{ const c: number[] = mkAcc(9); T("S join acc", () => S_join(c), c); }
{ const h = new F_join(); h.k = mkAcc(9); T("F join acc", () => h.run(), h.k); }
{ G = mkAcc(9); T("G join acc", () => G_join(), G); }
{ const c: number[] = mkAcc(9); T("L join acc", () => c.join("-"), c); }
{ const c: number[] = mkProto(9); T("P join proto", () => P_join(c), c); }
{ const c: number[] = mkProto(9); T("S join proto", () => S_join(c), c); }
{ const h = new F_join(); h.k = mkProto(9); T("F join proto", () => h.run(), h.k); }
{ G = mkProto(9); T("G join proto", () => G_join(), G); }
{ const c: number[] = mkProto(9); T("L join proto", () => c.join("-"), c); }
{ const c: number[] = mkSub(9); T("P join sub", () => P_join(c), c); }
{ const c: number[] = mkSub(9); T("S join sub", () => S_join(c), c); }
{ const h = new F_join(); h.k = mkSub(9); T("F join sub", () => h.run(), h.k); }
{ G = mkSub(9); T("G join sub", () => G_join(), G); }
{ const c: number[] = mkSub(9); T("L join sub", () => c.join("-"), c); }
{ const c: number[] = mkFake(9); T("P join fake", () => P_join(c), c); }
{ const c: number[] = mkFake(9); T("S join fake", () => S_join(c), c); }
{ const h = new F_join(); h.k = mkFake(9); T("F join fake", () => h.run(), h.k); }
{ G = mkFake(9); T("G join fake", () => G_join(), G); }
{ const c: number[] = mkFake(9); T("L join fake", () => c.join("-"), c); }
{ const c: number[] = mkNone(9); T("P join none", () => P_join(c), c); }
{ const c: number[] = mkNone(9); T("S join none", () => S_join(c), c); }
{ const h = new F_join(); h.k = mkNone(9); T("F join none", () => h.run(), h.k); }
{ G = mkNone(9); T("G join none", () => G_join(), G); }
{ const c: number[] = mkNone(9); T("L join none", () => c.join("-"), c); }
{ const c: number[] = mkNamed(9); T("P join named", () => P_join(c), c); }
{ const c: number[] = mkNamed(9); T("S join named", () => S_join(c), c); }
{ const h = new F_join(); h.k = mkNamed(9); T("F join named", () => h.run(), h.k); }
{ G = mkNamed(9); T("G join named", () => G_join(), G); }
{ const c: number[] = mkNamed(9); T("L join named", () => c.join("-"), c); }
{ const c: number[] = mkLent(9); T("P join lent", () => P_join(c), c); }
{ const c: number[] = mkLent(9); T("S join lent", () => S_join(c), c); }
{ const h = new F_join(); h.k = mkLent(9); T("F join lent", () => h.run(), h.k); }
{ G = mkLent(9); T("G join lent", () => G_join(), G); }
{ const c: number[] = mkLent(9); T("L join lent", () => c.join("-"), c); }
{ const c: number[] = mk(); T("P join again", () => P_join(c), c); }
{ const c: number[] = mk(); T("S join again", () => S_join(c), c); }
{ const h = new F_join(); h.k = mk(); T("F join again", () => h.run(), h.k); }
{ G = mk(); T("G join again", () => G_join(), G); }
{ const c: number[] = mk(); T("L join again", () => c.join("-"), c); }
{ const c: number[] = mk(); T("P reverse plain", () => P_reverse(c), c); }
{ const c: number[] = mk(); T("S reverse plain", () => S_reverse(c), c); }
{ const h = new F_reverse(); h.k = mk(); T("F reverse plain", () => h.run(), h.k); }
{ G = mk(); T("G reverse plain", () => G_reverse(), G); }
{ const c: number[] = mk(); T("L reverse plain", () => c.reverse(), c); }
{ const c: number[] = mkOwn(10); T("P reverse own", () => P_reverse(c), c); }
{ const c: number[] = mkOwn(10); T("S reverse own", () => S_reverse(c), c); }
{ const h = new F_reverse(); h.k = mkOwn(10); T("F reverse own", () => h.run(), h.k); }
{ G = mkOwn(10); T("G reverse own", () => G_reverse(), G); }
{ const c: number[] = mkOwn(10); T("L reverse own", () => c.reverse(), c); }
{ const c: number[] = mkAcc(10); T("P reverse acc", () => P_reverse(c), c); }
{ const c: number[] = mkAcc(10); T("S reverse acc", () => S_reverse(c), c); }
{ const h = new F_reverse(); h.k = mkAcc(10); T("F reverse acc", () => h.run(), h.k); }
{ G = mkAcc(10); T("G reverse acc", () => G_reverse(), G); }
{ const c: number[] = mkAcc(10); T("L reverse acc", () => c.reverse(), c); }
{ const c: number[] = mkProto(10); T("P reverse proto", () => P_reverse(c), c); }
{ const c: number[] = mkProto(10); T("S reverse proto", () => S_reverse(c), c); }
{ const h = new F_reverse(); h.k = mkProto(10); T("F reverse proto", () => h.run(), h.k); }
{ G = mkProto(10); T("G reverse proto", () => G_reverse(), G); }
{ const c: number[] = mkProto(10); T("L reverse proto", () => c.reverse(), c); }
{ const c: number[] = mkSub(10); T("P reverse sub", () => P_reverse(c), c); }
{ const c: number[] = mkSub(10); T("S reverse sub", () => S_reverse(c), c); }
{ const h = new F_reverse(); h.k = mkSub(10); T("F reverse sub", () => h.run(), h.k); }
{ G = mkSub(10); T("G reverse sub", () => G_reverse(), G); }
{ const c: number[] = mkSub(10); T("L reverse sub", () => c.reverse(), c); }
{ const c: number[] = mkFake(10); T("P reverse fake", () => P_reverse(c), c); }
{ const c: number[] = mkFake(10); T("S reverse fake", () => S_reverse(c), c); }
{ const h = new F_reverse(); h.k = mkFake(10); T("F reverse fake", () => h.run(), h.k); }
{ G = mkFake(10); T("G reverse fake", () => G_reverse(), G); }
{ const c: number[] = mkFake(10); T("L reverse fake", () => c.reverse(), c); }
{ const c: number[] = mkNone(10); T("P reverse none", () => P_reverse(c), c); }
{ const c: number[] = mkNone(10); T("S reverse none", () => S_reverse(c), c); }
{ const h = new F_reverse(); h.k = mkNone(10); T("F reverse none", () => h.run(), h.k); }
{ G = mkNone(10); T("G reverse none", () => G_reverse(), G); }
{ const c: number[] = mkNone(10); T("L reverse none", () => c.reverse(), c); }
{ const c: number[] = mkNamed(10); T("P reverse named", () => P_reverse(c), c); }
{ const c: number[] = mkNamed(10); T("S reverse named", () => S_reverse(c), c); }
{ const h = new F_reverse(); h.k = mkNamed(10); T("F reverse named", () => h.run(), h.k); }
{ G = mkNamed(10); T("G reverse named", () => G_reverse(), G); }
{ const c: number[] = mkNamed(10); T("L reverse named", () => c.reverse(), c); }
{ const c: number[] = mkLent(10); T("P reverse lent", () => P_reverse(c), c); }
{ const c: number[] = mkLent(10); T("S reverse lent", () => S_reverse(c), c); }
{ const h = new F_reverse(); h.k = mkLent(10); T("F reverse lent", () => h.run(), h.k); }
{ G = mkLent(10); T("G reverse lent", () => G_reverse(), G); }
{ const c: number[] = mkLent(10); T("L reverse lent", () => c.reverse(), c); }
{ const c: number[] = mk(); T("P reverse again", () => P_reverse(c), c); }
{ const c: number[] = mk(); T("S reverse again", () => S_reverse(c), c); }
{ const h = new F_reverse(); h.k = mk(); T("F reverse again", () => h.run(), h.k); }
{ G = mk(); T("G reverse again", () => G_reverse(), G); }
{ const c: number[] = mk(); T("L reverse again", () => c.reverse(), c); }
{ const c: number[] = mk(); T("P fill plain", () => P_fill(c), c); }
{ const c: number[] = mk(); T("S fill plain", () => S_fill(c), c); }
{ const h = new F_fill(); h.k = mk(); T("F fill plain", () => h.run(), h.k); }
{ G = mk(); T("G fill plain", () => G_fill(), G); }
{ const c: number[] = mk(); T("L fill plain", () => c.fill(5), c); }
{ const c: number[] = mkOwn(11); T("P fill own", () => P_fill(c), c); }
{ const c: number[] = mkOwn(11); T("S fill own", () => S_fill(c), c); }
{ const h = new F_fill(); h.k = mkOwn(11); T("F fill own", () => h.run(), h.k); }
{ G = mkOwn(11); T("G fill own", () => G_fill(), G); }
{ const c: number[] = mkOwn(11); T("L fill own", () => c.fill(5), c); }
{ const c: number[] = mkAcc(11); T("P fill acc", () => P_fill(c), c); }
{ const c: number[] = mkAcc(11); T("S fill acc", () => S_fill(c), c); }
{ const h = new F_fill(); h.k = mkAcc(11); T("F fill acc", () => h.run(), h.k); }
{ G = mkAcc(11); T("G fill acc", () => G_fill(), G); }
{ const c: number[] = mkAcc(11); T("L fill acc", () => c.fill(5), c); }
{ const c: number[] = mkProto(11); T("P fill proto", () => P_fill(c), c); }
{ const c: number[] = mkProto(11); T("S fill proto", () => S_fill(c), c); }
{ const h = new F_fill(); h.k = mkProto(11); T("F fill proto", () => h.run(), h.k); }
{ G = mkProto(11); T("G fill proto", () => G_fill(), G); }
{ const c: number[] = mkProto(11); T("L fill proto", () => c.fill(5), c); }
{ const c: number[] = mkSub(11); T("P fill sub", () => P_fill(c), c); }
{ const c: number[] = mkSub(11); T("S fill sub", () => S_fill(c), c); }
{ const h = new F_fill(); h.k = mkSub(11); T("F fill sub", () => h.run(), h.k); }
{ G = mkSub(11); T("G fill sub", () => G_fill(), G); }
{ const c: number[] = mkSub(11); T("L fill sub", () => c.fill(5), c); }
{ const c: number[] = mkFake(11); T("P fill fake", () => P_fill(c), c); }
{ const c: number[] = mkFake(11); T("S fill fake", () => S_fill(c), c); }
{ const h = new F_fill(); h.k = mkFake(11); T("F fill fake", () => h.run(), h.k); }
{ G = mkFake(11); T("G fill fake", () => G_fill(), G); }
{ const c: number[] = mkFake(11); T("L fill fake", () => c.fill(5), c); }
{ const c: number[] = mkNone(11); T("P fill none", () => P_fill(c), c); }
{ const c: number[] = mkNone(11); T("S fill none", () => S_fill(c), c); }
{ const h = new F_fill(); h.k = mkNone(11); T("F fill none", () => h.run(), h.k); }
{ G = mkNone(11); T("G fill none", () => G_fill(), G); }
{ const c: number[] = mkNone(11); T("L fill none", () => c.fill(5), c); }
{ const c: number[] = mkNamed(11); T("P fill named", () => P_fill(c), c); }
{ const c: number[] = mkNamed(11); T("S fill named", () => S_fill(c), c); }
{ const h = new F_fill(); h.k = mkNamed(11); T("F fill named", () => h.run(), h.k); }
{ G = mkNamed(11); T("G fill named", () => G_fill(), G); }
{ const c: number[] = mkNamed(11); T("L fill named", () => c.fill(5), c); }
{ const c: number[] = mkLent(11); T("P fill lent", () => P_fill(c), c); }
{ const c: number[] = mkLent(11); T("S fill lent", () => S_fill(c), c); }
{ const h = new F_fill(); h.k = mkLent(11); T("F fill lent", () => h.run(), h.k); }
{ G = mkLent(11); T("G fill lent", () => G_fill(), G); }
{ const c: number[] = mkLent(11); T("L fill lent", () => c.fill(5), c); }
{ const c: number[] = mk(); T("P fill again", () => P_fill(c), c); }
{ const c: number[] = mk(); T("S fill again", () => S_fill(c), c); }
{ const h = new F_fill(); h.k = mk(); T("F fill again", () => h.run(), h.k); }
{ G = mk(); T("G fill again", () => G_fill(), G); }
{ const c: number[] = mk(); T("L fill again", () => c.fill(5), c); }
{ const c: number[] = mk(); T("P at plain", () => P_at(c), c); }
{ const c: number[] = mk(); T("S at plain", () => S_at(c), c); }
{ const h = new F_at(); h.k = mk(); T("F at plain", () => h.run(), h.k); }
{ G = mk(); T("G at plain", () => G_at(), G); }
{ const c: number[] = mk(); T("L at plain", () => c.at(0), c); }
{ const c: number[] = mkOwn(12); T("P at own", () => P_at(c), c); }
{ const c: number[] = mkOwn(12); T("S at own", () => S_at(c), c); }
{ const h = new F_at(); h.k = mkOwn(12); T("F at own", () => h.run(), h.k); }
{ G = mkOwn(12); T("G at own", () => G_at(), G); }
{ const c: number[] = mkOwn(12); T("L at own", () => c.at(0), c); }
{ const c: number[] = mkAcc(12); T("P at acc", () => P_at(c), c); }
{ const c: number[] = mkAcc(12); T("S at acc", () => S_at(c), c); }
{ const h = new F_at(); h.k = mkAcc(12); T("F at acc", () => h.run(), h.k); }
{ G = mkAcc(12); T("G at acc", () => G_at(), G); }
{ const c: number[] = mkAcc(12); T("L at acc", () => c.at(0), c); }
{ const c: number[] = mkProto(12); T("P at proto", () => P_at(c), c); }
{ const c: number[] = mkProto(12); T("S at proto", () => S_at(c), c); }
{ const h = new F_at(); h.k = mkProto(12); T("F at proto", () => h.run(), h.k); }
{ G = mkProto(12); T("G at proto", () => G_at(), G); }
{ const c: number[] = mkProto(12); T("L at proto", () => c.at(0), c); }
{ const c: number[] = mkSub(12); T("P at sub", () => P_at(c), c); }
{ const c: number[] = mkSub(12); T("S at sub", () => S_at(c), c); }
{ const h = new F_at(); h.k = mkSub(12); T("F at sub", () => h.run(), h.k); }
{ G = mkSub(12); T("G at sub", () => G_at(), G); }
{ const c: number[] = mkSub(12); T("L at sub", () => c.at(0), c); }
{ const c: number[] = mkFake(12); T("P at fake", () => P_at(c), c); }
{ const c: number[] = mkFake(12); T("S at fake", () => S_at(c), c); }
{ const h = new F_at(); h.k = mkFake(12); T("F at fake", () => h.run(), h.k); }
{ G = mkFake(12); T("G at fake", () => G_at(), G); }
{ const c: number[] = mkFake(12); T("L at fake", () => c.at(0), c); }
{ const c: number[] = mkNone(12); T("P at none", () => P_at(c), c); }
{ const c: number[] = mkNone(12); T("S at none", () => S_at(c), c); }
{ const h = new F_at(); h.k = mkNone(12); T("F at none", () => h.run(), h.k); }
{ G = mkNone(12); T("G at none", () => G_at(), G); }
{ const c: number[] = mkNone(12); T("L at none", () => c.at(0), c); }
{ const c: number[] = mkNamed(12); T("P at named", () => P_at(c), c); }
{ const c: number[] = mkNamed(12); T("S at named", () => S_at(c), c); }
{ const h = new F_at(); h.k = mkNamed(12); T("F at named", () => h.run(), h.k); }
{ G = mkNamed(12); T("G at named", () => G_at(), G); }
{ const c: number[] = mkNamed(12); T("L at named", () => c.at(0), c); }
{ const c: number[] = mkLent(12); T("P at lent", () => P_at(c), c); }
{ const c: number[] = mkLent(12); T("S at lent", () => S_at(c), c); }
{ const h = new F_at(); h.k = mkLent(12); T("F at lent", () => h.run(), h.k); }
{ G = mkLent(12); T("G at lent", () => G_at(), G); }
{ const c: number[] = mkLent(12); T("L at lent", () => c.at(0), c); }
{ const c: number[] = mk(); T("P at again", () => P_at(c), c); }
{ const c: number[] = mk(); T("S at again", () => S_at(c), c); }
{ const h = new F_at(); h.k = mk(); T("F at again", () => h.run(), h.k); }
{ G = mk(); T("G at again", () => G_at(), G); }
{ const c: number[] = mk(); T("L at again", () => c.at(0), c); }
{ const c: number[] = mk(); T("P map plain", () => P_map(c), c); }
{ const c: number[] = mk(); T("S map plain", () => S_map(c), c); }
{ const h = new F_map(); h.k = mk(); T("F map plain", () => h.run(), h.k); }
{ G = mk(); T("G map plain", () => G_map(), G); }
{ const c: number[] = mk(); T("L map plain", () => c.map((x: number) => x + 1), c); }
{ const c: number[] = mkOwn(13); T("P map own", () => P_map(c), c); }
{ const c: number[] = mkOwn(13); T("S map own", () => S_map(c), c); }
{ const h = new F_map(); h.k = mkOwn(13); T("F map own", () => h.run(), h.k); }
{ G = mkOwn(13); T("G map own", () => G_map(), G); }
{ const c: number[] = mkOwn(13); T("L map own", () => c.map((x: number) => x + 1), c); }
{ const c: number[] = mkAcc(13); T("P map acc", () => P_map(c), c); }
{ const c: number[] = mkAcc(13); T("S map acc", () => S_map(c), c); }
{ const h = new F_map(); h.k = mkAcc(13); T("F map acc", () => h.run(), h.k); }
{ G = mkAcc(13); T("G map acc", () => G_map(), G); }
{ const c: number[] = mkAcc(13); T("L map acc", () => c.map((x: number) => x + 1), c); }
{ const c: number[] = mkProto(13); T("P map proto", () => P_map(c), c); }
{ const c: number[] = mkProto(13); T("S map proto", () => S_map(c), c); }
{ const h = new F_map(); h.k = mkProto(13); T("F map proto", () => h.run(), h.k); }
{ G = mkProto(13); T("G map proto", () => G_map(), G); }
{ const c: number[] = mkProto(13); T("L map proto", () => c.map((x: number) => x + 1), c); }
{ const c: number[] = mkSub(13); T("P map sub", () => P_map(c), c); }
{ const c: number[] = mkSub(13); T("S map sub", () => S_map(c), c); }
{ const h = new F_map(); h.k = mkSub(13); T("F map sub", () => h.run(), h.k); }
{ G = mkSub(13); T("G map sub", () => G_map(), G); }
{ const c: number[] = mkSub(13); T("L map sub", () => c.map((x: number) => x + 1), c); }
{ const c: number[] = mkFake(13); T("P map fake", () => P_map(c), c); }
{ const c: number[] = mkFake(13); T("S map fake", () => S_map(c), c); }
{ const h = new F_map(); h.k = mkFake(13); T("F map fake", () => h.run(), h.k); }
{ G = mkFake(13); T("G map fake", () => G_map(), G); }
{ const c: number[] = mkFake(13); T("L map fake", () => c.map((x: number) => x + 1), c); }
{ const c: number[] = mkNone(13); T("P map none", () => P_map(c), c); }
{ const c: number[] = mkNone(13); T("S map none", () => S_map(c), c); }
{ const h = new F_map(); h.k = mkNone(13); T("F map none", () => h.run(), h.k); }
{ G = mkNone(13); T("G map none", () => G_map(), G); }
{ const c: number[] = mkNone(13); T("L map none", () => c.map((x: number) => x + 1), c); }
{ const c: number[] = mkNamed(13); T("P map named", () => P_map(c), c); }
{ const c: number[] = mkNamed(13); T("S map named", () => S_map(c), c); }
{ const h = new F_map(); h.k = mkNamed(13); T("F map named", () => h.run(), h.k); }
{ G = mkNamed(13); T("G map named", () => G_map(), G); }
{ const c: number[] = mkNamed(13); T("L map named", () => c.map((x: number) => x + 1), c); }
{ const c: number[] = mkLent(13); T("P map lent", () => P_map(c), c); }
{ const c: number[] = mkLent(13); T("S map lent", () => S_map(c), c); }
{ const h = new F_map(); h.k = mkLent(13); T("F map lent", () => h.run(), h.k); }
{ G = mkLent(13); T("G map lent", () => G_map(), G); }
{ const c: number[] = mkLent(13); T("L map lent", () => c.map((x: number) => x + 1), c); }
{ const c: number[] = mk(); T("P map again", () => P_map(c), c); }
{ const c: number[] = mk(); T("S map again", () => S_map(c), c); }
{ const h = new F_map(); h.k = mk(); T("F map again", () => h.run(), h.k); }
{ G = mk(); T("G map again", () => G_map(), G); }
{ const c: number[] = mk(); T("L map again", () => c.map((x: number) => x + 1), c); }
{ const c: number[] = mk(); T("P filter plain", () => P_filter(c), c); }
{ const c: number[] = mk(); T("S filter plain", () => S_filter(c), c); }
{ const h = new F_filter(); h.k = mk(); T("F filter plain", () => h.run(), h.k); }
{ G = mk(); T("G filter plain", () => G_filter(), G); }
{ const c: number[] = mk(); T("L filter plain", () => c.filter((x: number) => x > 1), c); }
{ const c: number[] = mkOwn(14); T("P filter own", () => P_filter(c), c); }
{ const c: number[] = mkOwn(14); T("S filter own", () => S_filter(c), c); }
{ const h = new F_filter(); h.k = mkOwn(14); T("F filter own", () => h.run(), h.k); }
{ G = mkOwn(14); T("G filter own", () => G_filter(), G); }
{ const c: number[] = mkOwn(14); T("L filter own", () => c.filter((x: number) => x > 1), c); }
{ const c: number[] = mkAcc(14); T("P filter acc", () => P_filter(c), c); }
{ const c: number[] = mkAcc(14); T("S filter acc", () => S_filter(c), c); }
{ const h = new F_filter(); h.k = mkAcc(14); T("F filter acc", () => h.run(), h.k); }
{ G = mkAcc(14); T("G filter acc", () => G_filter(), G); }
{ const c: number[] = mkAcc(14); T("L filter acc", () => c.filter((x: number) => x > 1), c); }
{ const c: number[] = mkProto(14); T("P filter proto", () => P_filter(c), c); }
{ const c: number[] = mkProto(14); T("S filter proto", () => S_filter(c), c); }
{ const h = new F_filter(); h.k = mkProto(14); T("F filter proto", () => h.run(), h.k); }
{ G = mkProto(14); T("G filter proto", () => G_filter(), G); }
{ const c: number[] = mkProto(14); T("L filter proto", () => c.filter((x: number) => x > 1), c); }
{ const c: number[] = mkSub(14); T("P filter sub", () => P_filter(c), c); }
{ const c: number[] = mkSub(14); T("S filter sub", () => S_filter(c), c); }
{ const h = new F_filter(); h.k = mkSub(14); T("F filter sub", () => h.run(), h.k); }
{ G = mkSub(14); T("G filter sub", () => G_filter(), G); }
{ const c: number[] = mkSub(14); T("L filter sub", () => c.filter((x: number) => x > 1), c); }
{ const c: number[] = mkFake(14); T("P filter fake", () => P_filter(c), c); }
{ const c: number[] = mkFake(14); T("S filter fake", () => S_filter(c), c); }
{ const h = new F_filter(); h.k = mkFake(14); T("F filter fake", () => h.run(), h.k); }
{ G = mkFake(14); T("G filter fake", () => G_filter(), G); }
{ const c: number[] = mkFake(14); T("L filter fake", () => c.filter((x: number) => x > 1), c); }
{ const c: number[] = mkNone(14); T("P filter none", () => P_filter(c), c); }
{ const c: number[] = mkNone(14); T("S filter none", () => S_filter(c), c); }
{ const h = new F_filter(); h.k = mkNone(14); T("F filter none", () => h.run(), h.k); }
{ G = mkNone(14); T("G filter none", () => G_filter(), G); }
{ const c: number[] = mkNone(14); T("L filter none", () => c.filter((x: number) => x > 1), c); }
{ const c: number[] = mkNamed(14); T("P filter named", () => P_filter(c), c); }
{ const c: number[] = mkNamed(14); T("S filter named", () => S_filter(c), c); }
{ const h = new F_filter(); h.k = mkNamed(14); T("F filter named", () => h.run(), h.k); }
{ G = mkNamed(14); T("G filter named", () => G_filter(), G); }
{ const c: number[] = mkNamed(14); T("L filter named", () => c.filter((x: number) => x > 1), c); }
{ const c: number[] = mkLent(14); T("P filter lent", () => P_filter(c), c); }
{ const c: number[] = mkLent(14); T("S filter lent", () => S_filter(c), c); }
{ const h = new F_filter(); h.k = mkLent(14); T("F filter lent", () => h.run(), h.k); }
{ G = mkLent(14); T("G filter lent", () => G_filter(), G); }
{ const c: number[] = mkLent(14); T("L filter lent", () => c.filter((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P filter again", () => P_filter(c), c); }
{ const c: number[] = mk(); T("S filter again", () => S_filter(c), c); }
{ const h = new F_filter(); h.k = mk(); T("F filter again", () => h.run(), h.k); }
{ G = mk(); T("G filter again", () => G_filter(), G); }
{ const c: number[] = mk(); T("L filter again", () => c.filter((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P forEach plain", () => P_forEach(c), c); }
{ const c: number[] = mk(); T("S forEach plain", () => S_forEach(c), c); }
{ const h = new F_forEach(); h.k = mk(); T("F forEach plain", () => h.run(), h.k); }
{ G = mk(); T("G forEach plain", () => G_forEach(), G); }
{ const c: number[] = mk(); T("L forEach plain", () => c.forEach((x: number) => { sink += x; }), c); }
{ const c: number[] = mkOwn(15); T("P forEach own", () => P_forEach(c), c); }
{ const c: number[] = mkOwn(15); T("S forEach own", () => S_forEach(c), c); }
{ const h = new F_forEach(); h.k = mkOwn(15); T("F forEach own", () => h.run(), h.k); }
{ G = mkOwn(15); T("G forEach own", () => G_forEach(), G); }
{ const c: number[] = mkOwn(15); T("L forEach own", () => c.forEach((x: number) => { sink += x; }), c); }
{ const c: number[] = mkAcc(15); T("P forEach acc", () => P_forEach(c), c); }
{ const c: number[] = mkAcc(15); T("S forEach acc", () => S_forEach(c), c); }
{ const h = new F_forEach(); h.k = mkAcc(15); T("F forEach acc", () => h.run(), h.k); }
{ G = mkAcc(15); T("G forEach acc", () => G_forEach(), G); }
{ const c: number[] = mkAcc(15); T("L forEach acc", () => c.forEach((x: number) => { sink += x; }), c); }
{ const c: number[] = mkProto(15); T("P forEach proto", () => P_forEach(c), c); }
{ const c: number[] = mkProto(15); T("S forEach proto", () => S_forEach(c), c); }
{ const h = new F_forEach(); h.k = mkProto(15); T("F forEach proto", () => h.run(), h.k); }
{ G = mkProto(15); T("G forEach proto", () => G_forEach(), G); }
{ const c: number[] = mkProto(15); T("L forEach proto", () => c.forEach((x: number) => { sink += x; }), c); }
{ const c: number[] = mkSub(15); T("P forEach sub", () => P_forEach(c), c); }
{ const c: number[] = mkSub(15); T("S forEach sub", () => S_forEach(c), c); }
{ const h = new F_forEach(); h.k = mkSub(15); T("F forEach sub", () => h.run(), h.k); }
{ G = mkSub(15); T("G forEach sub", () => G_forEach(), G); }
{ const c: number[] = mkSub(15); T("L forEach sub", () => c.forEach((x: number) => { sink += x; }), c); }
{ const c: number[] = mkFake(15); T("P forEach fake", () => P_forEach(c), c); }
{ const c: number[] = mkFake(15); T("S forEach fake", () => S_forEach(c), c); }
{ const h = new F_forEach(); h.k = mkFake(15); T("F forEach fake", () => h.run(), h.k); }
{ G = mkFake(15); T("G forEach fake", () => G_forEach(), G); }
{ const c: number[] = mkFake(15); T("L forEach fake", () => c.forEach((x: number) => { sink += x; }), c); }
{ const c: number[] = mkNone(15); T("P forEach none", () => P_forEach(c), c); }
{ const c: number[] = mkNone(15); T("S forEach none", () => S_forEach(c), c); }
{ const h = new F_forEach(); h.k = mkNone(15); T("F forEach none", () => h.run(), h.k); }
{ G = mkNone(15); T("G forEach none", () => G_forEach(), G); }
{ const c: number[] = mkNone(15); T("L forEach none", () => c.forEach((x: number) => { sink += x; }), c); }
{ const c: number[] = mkNamed(15); T("P forEach named", () => P_forEach(c), c); }
{ const c: number[] = mkNamed(15); T("S forEach named", () => S_forEach(c), c); }
{ const h = new F_forEach(); h.k = mkNamed(15); T("F forEach named", () => h.run(), h.k); }
{ G = mkNamed(15); T("G forEach named", () => G_forEach(), G); }
{ const c: number[] = mkNamed(15); T("L forEach named", () => c.forEach((x: number) => { sink += x; }), c); }
{ const c: number[] = mkLent(15); T("P forEach lent", () => P_forEach(c), c); }
{ const c: number[] = mkLent(15); T("S forEach lent", () => S_forEach(c), c); }
{ const h = new F_forEach(); h.k = mkLent(15); T("F forEach lent", () => h.run(), h.k); }
{ G = mkLent(15); T("G forEach lent", () => G_forEach(), G); }
{ const c: number[] = mkLent(15); T("L forEach lent", () => c.forEach((x: number) => { sink += x; }), c); }
{ const c: number[] = mk(); T("P forEach again", () => P_forEach(c), c); }
{ const c: number[] = mk(); T("S forEach again", () => S_forEach(c), c); }
{ const h = new F_forEach(); h.k = mk(); T("F forEach again", () => h.run(), h.k); }
{ G = mk(); T("G forEach again", () => G_forEach(), G); }
{ const c: number[] = mk(); T("L forEach again", () => c.forEach((x: number) => { sink += x; }), c); }
{ const c: number[] = mk(); T("P some plain", () => P_some(c), c); }
{ const c: number[] = mk(); T("S some plain", () => S_some(c), c); }
{ const h = new F_some(); h.k = mk(); T("F some plain", () => h.run(), h.k); }
{ G = mk(); T("G some plain", () => G_some(), G); }
{ const c: number[] = mk(); T("L some plain", () => c.some((x: number) => x > 1), c); }
{ const c: number[] = mkOwn(16); T("P some own", () => P_some(c), c); }
{ const c: number[] = mkOwn(16); T("S some own", () => S_some(c), c); }
{ const h = new F_some(); h.k = mkOwn(16); T("F some own", () => h.run(), h.k); }
{ G = mkOwn(16); T("G some own", () => G_some(), G); }
{ const c: number[] = mkOwn(16); T("L some own", () => c.some((x: number) => x > 1), c); }
{ const c: number[] = mkAcc(16); T("P some acc", () => P_some(c), c); }
{ const c: number[] = mkAcc(16); T("S some acc", () => S_some(c), c); }
{ const h = new F_some(); h.k = mkAcc(16); T("F some acc", () => h.run(), h.k); }
{ G = mkAcc(16); T("G some acc", () => G_some(), G); }
{ const c: number[] = mkAcc(16); T("L some acc", () => c.some((x: number) => x > 1), c); }
{ const c: number[] = mkProto(16); T("P some proto", () => P_some(c), c); }
{ const c: number[] = mkProto(16); T("S some proto", () => S_some(c), c); }
{ const h = new F_some(); h.k = mkProto(16); T("F some proto", () => h.run(), h.k); }
{ G = mkProto(16); T("G some proto", () => G_some(), G); }
{ const c: number[] = mkProto(16); T("L some proto", () => c.some((x: number) => x > 1), c); }
{ const c: number[] = mkSub(16); T("P some sub", () => P_some(c), c); }
{ const c: number[] = mkSub(16); T("S some sub", () => S_some(c), c); }
{ const h = new F_some(); h.k = mkSub(16); T("F some sub", () => h.run(), h.k); }
{ G = mkSub(16); T("G some sub", () => G_some(), G); }
{ const c: number[] = mkSub(16); T("L some sub", () => c.some((x: number) => x > 1), c); }
{ const c: number[] = mkFake(16); T("P some fake", () => P_some(c), c); }
{ const c: number[] = mkFake(16); T("S some fake", () => S_some(c), c); }
{ const h = new F_some(); h.k = mkFake(16); T("F some fake", () => h.run(), h.k); }
{ G = mkFake(16); T("G some fake", () => G_some(), G); }
{ const c: number[] = mkFake(16); T("L some fake", () => c.some((x: number) => x > 1), c); }
{ const c: number[] = mkNone(16); T("P some none", () => P_some(c), c); }
{ const c: number[] = mkNone(16); T("S some none", () => S_some(c), c); }
{ const h = new F_some(); h.k = mkNone(16); T("F some none", () => h.run(), h.k); }
{ G = mkNone(16); T("G some none", () => G_some(), G); }
{ const c: number[] = mkNone(16); T("L some none", () => c.some((x: number) => x > 1), c); }
{ const c: number[] = mkNamed(16); T("P some named", () => P_some(c), c); }
{ const c: number[] = mkNamed(16); T("S some named", () => S_some(c), c); }
{ const h = new F_some(); h.k = mkNamed(16); T("F some named", () => h.run(), h.k); }
{ G = mkNamed(16); T("G some named", () => G_some(), G); }
{ const c: number[] = mkNamed(16); T("L some named", () => c.some((x: number) => x > 1), c); }
{ const c: number[] = mkLent(16); T("P some lent", () => P_some(c), c); }
{ const c: number[] = mkLent(16); T("S some lent", () => S_some(c), c); }
{ const h = new F_some(); h.k = mkLent(16); T("F some lent", () => h.run(), h.k); }
{ G = mkLent(16); T("G some lent", () => G_some(), G); }
{ const c: number[] = mkLent(16); T("L some lent", () => c.some((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P some again", () => P_some(c), c); }
{ const c: number[] = mk(); T("S some again", () => S_some(c), c); }
{ const h = new F_some(); h.k = mk(); T("F some again", () => h.run(), h.k); }
{ G = mk(); T("G some again", () => G_some(), G); }
{ const c: number[] = mk(); T("L some again", () => c.some((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P every plain", () => P_every(c), c); }
{ const c: number[] = mk(); T("S every plain", () => S_every(c), c); }
{ const h = new F_every(); h.k = mk(); T("F every plain", () => h.run(), h.k); }
{ G = mk(); T("G every plain", () => G_every(), G); }
{ const c: number[] = mk(); T("L every plain", () => c.every((x: number) => x > 1), c); }
{ const c: number[] = mkOwn(17); T("P every own", () => P_every(c), c); }
{ const c: number[] = mkOwn(17); T("S every own", () => S_every(c), c); }
{ const h = new F_every(); h.k = mkOwn(17); T("F every own", () => h.run(), h.k); }
{ G = mkOwn(17); T("G every own", () => G_every(), G); }
{ const c: number[] = mkOwn(17); T("L every own", () => c.every((x: number) => x > 1), c); }
{ const c: number[] = mkAcc(17); T("P every acc", () => P_every(c), c); }
{ const c: number[] = mkAcc(17); T("S every acc", () => S_every(c), c); }
{ const h = new F_every(); h.k = mkAcc(17); T("F every acc", () => h.run(), h.k); }
{ G = mkAcc(17); T("G every acc", () => G_every(), G); }
{ const c: number[] = mkAcc(17); T("L every acc", () => c.every((x: number) => x > 1), c); }
{ const c: number[] = mkProto(17); T("P every proto", () => P_every(c), c); }
{ const c: number[] = mkProto(17); T("S every proto", () => S_every(c), c); }
{ const h = new F_every(); h.k = mkProto(17); T("F every proto", () => h.run(), h.k); }
{ G = mkProto(17); T("G every proto", () => G_every(), G); }
{ const c: number[] = mkProto(17); T("L every proto", () => c.every((x: number) => x > 1), c); }
{ const c: number[] = mkSub(17); T("P every sub", () => P_every(c), c); }
{ const c: number[] = mkSub(17); T("S every sub", () => S_every(c), c); }
{ const h = new F_every(); h.k = mkSub(17); T("F every sub", () => h.run(), h.k); }
{ G = mkSub(17); T("G every sub", () => G_every(), G); }
{ const c: number[] = mkSub(17); T("L every sub", () => c.every((x: number) => x > 1), c); }
{ const c: number[] = mkFake(17); T("P every fake", () => P_every(c), c); }
{ const c: number[] = mkFake(17); T("S every fake", () => S_every(c), c); }
{ const h = new F_every(); h.k = mkFake(17); T("F every fake", () => h.run(), h.k); }
{ G = mkFake(17); T("G every fake", () => G_every(), G); }
{ const c: number[] = mkFake(17); T("L every fake", () => c.every((x: number) => x > 1), c); }
{ const c: number[] = mkNone(17); T("P every none", () => P_every(c), c); }
{ const c: number[] = mkNone(17); T("S every none", () => S_every(c), c); }
{ const h = new F_every(); h.k = mkNone(17); T("F every none", () => h.run(), h.k); }
{ G = mkNone(17); T("G every none", () => G_every(), G); }
{ const c: number[] = mkNone(17); T("L every none", () => c.every((x: number) => x > 1), c); }
{ const c: number[] = mkNamed(17); T("P every named", () => P_every(c), c); }
{ const c: number[] = mkNamed(17); T("S every named", () => S_every(c), c); }
{ const h = new F_every(); h.k = mkNamed(17); T("F every named", () => h.run(), h.k); }
{ G = mkNamed(17); T("G every named", () => G_every(), G); }
{ const c: number[] = mkNamed(17); T("L every named", () => c.every((x: number) => x > 1), c); }
{ const c: number[] = mkLent(17); T("P every lent", () => P_every(c), c); }
{ const c: number[] = mkLent(17); T("S every lent", () => S_every(c), c); }
{ const h = new F_every(); h.k = mkLent(17); T("F every lent", () => h.run(), h.k); }
{ G = mkLent(17); T("G every lent", () => G_every(), G); }
{ const c: number[] = mkLent(17); T("L every lent", () => c.every((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P every again", () => P_every(c), c); }
{ const c: number[] = mk(); T("S every again", () => S_every(c), c); }
{ const h = new F_every(); h.k = mk(); T("F every again", () => h.run(), h.k); }
{ G = mk(); T("G every again", () => G_every(), G); }
{ const c: number[] = mk(); T("L every again", () => c.every((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P find plain", () => P_find(c), c); }
{ const c: number[] = mk(); T("S find plain", () => S_find(c), c); }
{ const h = new F_find(); h.k = mk(); T("F find plain", () => h.run(), h.k); }
{ G = mk(); T("G find plain", () => G_find(), G); }
{ const c: number[] = mk(); T("L find plain", () => c.find((x: number) => x > 1), c); }
{ const c: number[] = mkOwn(18); T("P find own", () => P_find(c), c); }
{ const c: number[] = mkOwn(18); T("S find own", () => S_find(c), c); }
{ const h = new F_find(); h.k = mkOwn(18); T("F find own", () => h.run(), h.k); }
{ G = mkOwn(18); T("G find own", () => G_find(), G); }
{ const c: number[] = mkOwn(18); T("L find own", () => c.find((x: number) => x > 1), c); }
{ const c: number[] = mkAcc(18); T("P find acc", () => P_find(c), c); }
{ const c: number[] = mkAcc(18); T("S find acc", () => S_find(c), c); }
{ const h = new F_find(); h.k = mkAcc(18); T("F find acc", () => h.run(), h.k); }
{ G = mkAcc(18); T("G find acc", () => G_find(), G); }
{ const c: number[] = mkAcc(18); T("L find acc", () => c.find((x: number) => x > 1), c); }
{ const c: number[] = mkProto(18); T("P find proto", () => P_find(c), c); }
{ const c: number[] = mkProto(18); T("S find proto", () => S_find(c), c); }
{ const h = new F_find(); h.k = mkProto(18); T("F find proto", () => h.run(), h.k); }
{ G = mkProto(18); T("G find proto", () => G_find(), G); }
{ const c: number[] = mkProto(18); T("L find proto", () => c.find((x: number) => x > 1), c); }
{ const c: number[] = mkSub(18); T("P find sub", () => P_find(c), c); }
{ const c: number[] = mkSub(18); T("S find sub", () => S_find(c), c); }
{ const h = new F_find(); h.k = mkSub(18); T("F find sub", () => h.run(), h.k); }
{ G = mkSub(18); T("G find sub", () => G_find(), G); }
{ const c: number[] = mkSub(18); T("L find sub", () => c.find((x: number) => x > 1), c); }
{ const c: number[] = mkFake(18); T("P find fake", () => P_find(c), c); }
{ const c: number[] = mkFake(18); T("S find fake", () => S_find(c), c); }
{ const h = new F_find(); h.k = mkFake(18); T("F find fake", () => h.run(), h.k); }
{ G = mkFake(18); T("G find fake", () => G_find(), G); }
{ const c: number[] = mkFake(18); T("L find fake", () => c.find((x: number) => x > 1), c); }
{ const c: number[] = mkNone(18); T("P find none", () => P_find(c), c); }
{ const c: number[] = mkNone(18); T("S find none", () => S_find(c), c); }
{ const h = new F_find(); h.k = mkNone(18); T("F find none", () => h.run(), h.k); }
{ G = mkNone(18); T("G find none", () => G_find(), G); }
{ const c: number[] = mkNone(18); T("L find none", () => c.find((x: number) => x > 1), c); }
{ const c: number[] = mkNamed(18); T("P find named", () => P_find(c), c); }
{ const c: number[] = mkNamed(18); T("S find named", () => S_find(c), c); }
{ const h = new F_find(); h.k = mkNamed(18); T("F find named", () => h.run(), h.k); }
{ G = mkNamed(18); T("G find named", () => G_find(), G); }
{ const c: number[] = mkNamed(18); T("L find named", () => c.find((x: number) => x > 1), c); }
{ const c: number[] = mkLent(18); T("P find lent", () => P_find(c), c); }
{ const c: number[] = mkLent(18); T("S find lent", () => S_find(c), c); }
{ const h = new F_find(); h.k = mkLent(18); T("F find lent", () => h.run(), h.k); }
{ G = mkLent(18); T("G find lent", () => G_find(), G); }
{ const c: number[] = mkLent(18); T("L find lent", () => c.find((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P find again", () => P_find(c), c); }
{ const c: number[] = mk(); T("S find again", () => S_find(c), c); }
{ const h = new F_find(); h.k = mk(); T("F find again", () => h.run(), h.k); }
{ G = mk(); T("G find again", () => G_find(), G); }
{ const c: number[] = mk(); T("L find again", () => c.find((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P findIndex plain", () => P_findIndex(c), c); }
{ const c: number[] = mk(); T("S findIndex plain", () => S_findIndex(c), c); }
{ const h = new F_findIndex(); h.k = mk(); T("F findIndex plain", () => h.run(), h.k); }
{ G = mk(); T("G findIndex plain", () => G_findIndex(), G); }
{ const c: number[] = mk(); T("L findIndex plain", () => c.findIndex((x: number) => x > 1), c); }
{ const c: number[] = mkOwn(19); T("P findIndex own", () => P_findIndex(c), c); }
{ const c: number[] = mkOwn(19); T("S findIndex own", () => S_findIndex(c), c); }
{ const h = new F_findIndex(); h.k = mkOwn(19); T("F findIndex own", () => h.run(), h.k); }
{ G = mkOwn(19); T("G findIndex own", () => G_findIndex(), G); }
{ const c: number[] = mkOwn(19); T("L findIndex own", () => c.findIndex((x: number) => x > 1), c); }
{ const c: number[] = mkAcc(19); T("P findIndex acc", () => P_findIndex(c), c); }
{ const c: number[] = mkAcc(19); T("S findIndex acc", () => S_findIndex(c), c); }
{ const h = new F_findIndex(); h.k = mkAcc(19); T("F findIndex acc", () => h.run(), h.k); }
{ G = mkAcc(19); T("G findIndex acc", () => G_findIndex(), G); }
{ const c: number[] = mkAcc(19); T("L findIndex acc", () => c.findIndex((x: number) => x > 1), c); }
{ const c: number[] = mkProto(19); T("P findIndex proto", () => P_findIndex(c), c); }
{ const c: number[] = mkProto(19); T("S findIndex proto", () => S_findIndex(c), c); }
{ const h = new F_findIndex(); h.k = mkProto(19); T("F findIndex proto", () => h.run(), h.k); }
{ G = mkProto(19); T("G findIndex proto", () => G_findIndex(), G); }
{ const c: number[] = mkProto(19); T("L findIndex proto", () => c.findIndex((x: number) => x > 1), c); }
{ const c: number[] = mkSub(19); T("P findIndex sub", () => P_findIndex(c), c); }
{ const c: number[] = mkSub(19); T("S findIndex sub", () => S_findIndex(c), c); }
{ const h = new F_findIndex(); h.k = mkSub(19); T("F findIndex sub", () => h.run(), h.k); }
{ G = mkSub(19); T("G findIndex sub", () => G_findIndex(), G); }
{ const c: number[] = mkSub(19); T("L findIndex sub", () => c.findIndex((x: number) => x > 1), c); }
{ const c: number[] = mkFake(19); T("P findIndex fake", () => P_findIndex(c), c); }
{ const c: number[] = mkFake(19); T("S findIndex fake", () => S_findIndex(c), c); }
{ const h = new F_findIndex(); h.k = mkFake(19); T("F findIndex fake", () => h.run(), h.k); }
{ G = mkFake(19); T("G findIndex fake", () => G_findIndex(), G); }
{ const c: number[] = mkFake(19); T("L findIndex fake", () => c.findIndex((x: number) => x > 1), c); }
{ const c: number[] = mkNone(19); T("P findIndex none", () => P_findIndex(c), c); }
{ const c: number[] = mkNone(19); T("S findIndex none", () => S_findIndex(c), c); }
{ const h = new F_findIndex(); h.k = mkNone(19); T("F findIndex none", () => h.run(), h.k); }
{ G = mkNone(19); T("G findIndex none", () => G_findIndex(), G); }
{ const c: number[] = mkNone(19); T("L findIndex none", () => c.findIndex((x: number) => x > 1), c); }
{ const c: number[] = mkNamed(19); T("P findIndex named", () => P_findIndex(c), c); }
{ const c: number[] = mkNamed(19); T("S findIndex named", () => S_findIndex(c), c); }
{ const h = new F_findIndex(); h.k = mkNamed(19); T("F findIndex named", () => h.run(), h.k); }
{ G = mkNamed(19); T("G findIndex named", () => G_findIndex(), G); }
{ const c: number[] = mkNamed(19); T("L findIndex named", () => c.findIndex((x: number) => x > 1), c); }
{ const c: number[] = mkLent(19); T("P findIndex lent", () => P_findIndex(c), c); }
{ const c: number[] = mkLent(19); T("S findIndex lent", () => S_findIndex(c), c); }
{ const h = new F_findIndex(); h.k = mkLent(19); T("F findIndex lent", () => h.run(), h.k); }
{ G = mkLent(19); T("G findIndex lent", () => G_findIndex(), G); }
{ const c: number[] = mkLent(19); T("L findIndex lent", () => c.findIndex((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P findIndex again", () => P_findIndex(c), c); }
{ const c: number[] = mk(); T("S findIndex again", () => S_findIndex(c), c); }
{ const h = new F_findIndex(); h.k = mk(); T("F findIndex again", () => h.run(), h.k); }
{ G = mk(); T("G findIndex again", () => G_findIndex(), G); }
{ const c: number[] = mk(); T("L findIndex again", () => c.findIndex((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P reduce plain", () => P_reduce(c), c); }
{ const c: number[] = mk(); T("S reduce plain", () => S_reduce(c), c); }
{ const h = new F_reduce(); h.k = mk(); T("F reduce plain", () => h.run(), h.k); }
{ G = mk(); T("G reduce plain", () => G_reduce(), G); }
{ const c: number[] = mk(); T("L reduce plain", () => c.reduce((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkOwn(20); T("P reduce own", () => P_reduce(c), c); }
{ const c: number[] = mkOwn(20); T("S reduce own", () => S_reduce(c), c); }
{ const h = new F_reduce(); h.k = mkOwn(20); T("F reduce own", () => h.run(), h.k); }
{ G = mkOwn(20); T("G reduce own", () => G_reduce(), G); }
{ const c: number[] = mkOwn(20); T("L reduce own", () => c.reduce((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkAcc(20); T("P reduce acc", () => P_reduce(c), c); }
{ const c: number[] = mkAcc(20); T("S reduce acc", () => S_reduce(c), c); }
{ const h = new F_reduce(); h.k = mkAcc(20); T("F reduce acc", () => h.run(), h.k); }
{ G = mkAcc(20); T("G reduce acc", () => G_reduce(), G); }
{ const c: number[] = mkAcc(20); T("L reduce acc", () => c.reduce((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkProto(20); T("P reduce proto", () => P_reduce(c), c); }
{ const c: number[] = mkProto(20); T("S reduce proto", () => S_reduce(c), c); }
{ const h = new F_reduce(); h.k = mkProto(20); T("F reduce proto", () => h.run(), h.k); }
{ G = mkProto(20); T("G reduce proto", () => G_reduce(), G); }
{ const c: number[] = mkProto(20); T("L reduce proto", () => c.reduce((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkSub(20); T("P reduce sub", () => P_reduce(c), c); }
{ const c: number[] = mkSub(20); T("S reduce sub", () => S_reduce(c), c); }
{ const h = new F_reduce(); h.k = mkSub(20); T("F reduce sub", () => h.run(), h.k); }
{ G = mkSub(20); T("G reduce sub", () => G_reduce(), G); }
{ const c: number[] = mkSub(20); T("L reduce sub", () => c.reduce((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkFake(20); T("P reduce fake", () => P_reduce(c), c); }
{ const c: number[] = mkFake(20); T("S reduce fake", () => S_reduce(c), c); }
{ const h = new F_reduce(); h.k = mkFake(20); T("F reduce fake", () => h.run(), h.k); }
{ G = mkFake(20); T("G reduce fake", () => G_reduce(), G); }
{ const c: number[] = mkFake(20); T("L reduce fake", () => c.reduce((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkNone(20); T("P reduce none", () => P_reduce(c), c); }
{ const c: number[] = mkNone(20); T("S reduce none", () => S_reduce(c), c); }
{ const h = new F_reduce(); h.k = mkNone(20); T("F reduce none", () => h.run(), h.k); }
{ G = mkNone(20); T("G reduce none", () => G_reduce(), G); }
{ const c: number[] = mkNone(20); T("L reduce none", () => c.reduce((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkNamed(20); T("P reduce named", () => P_reduce(c), c); }
{ const c: number[] = mkNamed(20); T("S reduce named", () => S_reduce(c), c); }
{ const h = new F_reduce(); h.k = mkNamed(20); T("F reduce named", () => h.run(), h.k); }
{ G = mkNamed(20); T("G reduce named", () => G_reduce(), G); }
{ const c: number[] = mkNamed(20); T("L reduce named", () => c.reduce((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkLent(20); T("P reduce lent", () => P_reduce(c), c); }
{ const c: number[] = mkLent(20); T("S reduce lent", () => S_reduce(c), c); }
{ const h = new F_reduce(); h.k = mkLent(20); T("F reduce lent", () => h.run(), h.k); }
{ G = mkLent(20); T("G reduce lent", () => G_reduce(), G); }
{ const c: number[] = mkLent(20); T("L reduce lent", () => c.reduce((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mk(); T("P reduce again", () => P_reduce(c), c); }
{ const c: number[] = mk(); T("S reduce again", () => S_reduce(c), c); }
{ const h = new F_reduce(); h.k = mk(); T("F reduce again", () => h.run(), h.k); }
{ G = mk(); T("G reduce again", () => G_reduce(), G); }
{ const c: number[] = mk(); T("L reduce again", () => c.reduce((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mk(); T("P sort plain", () => P_sort(c), c); }
{ const c: number[] = mk(); T("S sort plain", () => S_sort(c), c); }
{ const h = new F_sort(); h.k = mk(); T("F sort plain", () => h.run(), h.k); }
{ G = mk(); T("G sort plain", () => G_sort(), G); }
{ const c: number[] = mk(); T("L sort plain", () => c.sort((x: number, y: number) => x - y), c); }
{ const c: number[] = mkOwn(21); T("P sort own", () => P_sort(c), c); }
{ const c: number[] = mkOwn(21); T("S sort own", () => S_sort(c), c); }
{ const h = new F_sort(); h.k = mkOwn(21); T("F sort own", () => h.run(), h.k); }
{ G = mkOwn(21); T("G sort own", () => G_sort(), G); }
{ const c: number[] = mkOwn(21); T("L sort own", () => c.sort((x: number, y: number) => x - y), c); }
{ const c: number[] = mkAcc(21); T("P sort acc", () => P_sort(c), c); }
{ const c: number[] = mkAcc(21); T("S sort acc", () => S_sort(c), c); }
{ const h = new F_sort(); h.k = mkAcc(21); T("F sort acc", () => h.run(), h.k); }
{ G = mkAcc(21); T("G sort acc", () => G_sort(), G); }
{ const c: number[] = mkAcc(21); T("L sort acc", () => c.sort((x: number, y: number) => x - y), c); }
{ const c: number[] = mkProto(21); T("P sort proto", () => P_sort(c), c); }
{ const c: number[] = mkProto(21); T("S sort proto", () => S_sort(c), c); }
{ const h = new F_sort(); h.k = mkProto(21); T("F sort proto", () => h.run(), h.k); }
{ G = mkProto(21); T("G sort proto", () => G_sort(), G); }
{ const c: number[] = mkProto(21); T("L sort proto", () => c.sort((x: number, y: number) => x - y), c); }
{ const c: number[] = mkSub(21); T("P sort sub", () => P_sort(c), c); }
{ const c: number[] = mkSub(21); T("S sort sub", () => S_sort(c), c); }
{ const h = new F_sort(); h.k = mkSub(21); T("F sort sub", () => h.run(), h.k); }
{ G = mkSub(21); T("G sort sub", () => G_sort(), G); }
{ const c: number[] = mkSub(21); T("L sort sub", () => c.sort((x: number, y: number) => x - y), c); }
{ const c: number[] = mkFake(21); T("P sort fake", () => P_sort(c), c); }
{ const c: number[] = mkFake(21); T("S sort fake", () => S_sort(c), c); }
{ const h = new F_sort(); h.k = mkFake(21); T("F sort fake", () => h.run(), h.k); }
{ G = mkFake(21); T("G sort fake", () => G_sort(), G); }
{ const c: number[] = mkFake(21); T("L sort fake", () => c.sort((x: number, y: number) => x - y), c); }
{ const c: number[] = mkNone(21); T("P sort none", () => P_sort(c), c); }
{ const c: number[] = mkNone(21); T("S sort none", () => S_sort(c), c); }
{ const h = new F_sort(); h.k = mkNone(21); T("F sort none", () => h.run(), h.k); }
{ G = mkNone(21); T("G sort none", () => G_sort(), G); }
{ const c: number[] = mkNone(21); T("L sort none", () => c.sort((x: number, y: number) => x - y), c); }
{ const c: number[] = mkNamed(21); T("P sort named", () => P_sort(c), c); }
{ const c: number[] = mkNamed(21); T("S sort named", () => S_sort(c), c); }
{ const h = new F_sort(); h.k = mkNamed(21); T("F sort named", () => h.run(), h.k); }
{ G = mkNamed(21); T("G sort named", () => G_sort(), G); }
{ const c: number[] = mkNamed(21); T("L sort named", () => c.sort((x: number, y: number) => x - y), c); }
{ const c: number[] = mkLent(21); T("P sort lent", () => P_sort(c), c); }
{ const c: number[] = mkLent(21); T("S sort lent", () => S_sort(c), c); }
{ const h = new F_sort(); h.k = mkLent(21); T("F sort lent", () => h.run(), h.k); }
{ G = mkLent(21); T("G sort lent", () => G_sort(), G); }
{ const c: number[] = mkLent(21); T("L sort lent", () => c.sort((x: number, y: number) => x - y), c); }
{ const c: number[] = mk(); T("P sort again", () => P_sort(c), c); }
{ const c: number[] = mk(); T("S sort again", () => S_sort(c), c); }
{ const h = new F_sort(); h.k = mk(); T("F sort again", () => h.run(), h.k); }
{ G = mk(); T("G sort again", () => G_sort(), G); }
{ const c: number[] = mk(); T("L sort again", () => c.sort((x: number, y: number) => x - y), c); }
{ const c: number[] = mk(); T("P copyWithin plain", () => P_copyWithin(c), c); }
{ const c: number[] = mk(); T("S copyWithin plain", () => S_copyWithin(c), c); }
{ const h = new F_copyWithin(); h.k = mk(); T("F copyWithin plain", () => h.run(), h.k); }
{ G = mk(); T("G copyWithin plain", () => G_copyWithin(), G); }
{ const c: number[] = mk(); T("L copyWithin plain", () => c.copyWithin(0, 1), c); }
{ const c: number[] = mkOwn(22); T("P copyWithin own", () => P_copyWithin(c), c); }
{ const c: number[] = mkOwn(22); T("S copyWithin own", () => S_copyWithin(c), c); }
{ const h = new F_copyWithin(); h.k = mkOwn(22); T("F copyWithin own", () => h.run(), h.k); }
{ G = mkOwn(22); T("G copyWithin own", () => G_copyWithin(), G); }
{ const c: number[] = mkOwn(22); T("L copyWithin own", () => c.copyWithin(0, 1), c); }
{ const c: number[] = mkAcc(22); T("P copyWithin acc", () => P_copyWithin(c), c); }
{ const c: number[] = mkAcc(22); T("S copyWithin acc", () => S_copyWithin(c), c); }
{ const h = new F_copyWithin(); h.k = mkAcc(22); T("F copyWithin acc", () => h.run(), h.k); }
{ G = mkAcc(22); T("G copyWithin acc", () => G_copyWithin(), G); }
{ const c: number[] = mkAcc(22); T("L copyWithin acc", () => c.copyWithin(0, 1), c); }
{ const c: number[] = mkProto(22); T("P copyWithin proto", () => P_copyWithin(c), c); }
{ const c: number[] = mkProto(22); T("S copyWithin proto", () => S_copyWithin(c), c); }
{ const h = new F_copyWithin(); h.k = mkProto(22); T("F copyWithin proto", () => h.run(), h.k); }
{ G = mkProto(22); T("G copyWithin proto", () => G_copyWithin(), G); }
{ const c: number[] = mkProto(22); T("L copyWithin proto", () => c.copyWithin(0, 1), c); }
{ const c: number[] = mkSub(22); T("P copyWithin sub", () => P_copyWithin(c), c); }
{ const c: number[] = mkSub(22); T("S copyWithin sub", () => S_copyWithin(c), c); }
{ const h = new F_copyWithin(); h.k = mkSub(22); T("F copyWithin sub", () => h.run(), h.k); }
{ G = mkSub(22); T("G copyWithin sub", () => G_copyWithin(), G); }
{ const c: number[] = mkSub(22); T("L copyWithin sub", () => c.copyWithin(0, 1), c); }
{ const c: number[] = mkFake(22); T("P copyWithin fake", () => P_copyWithin(c), c); }
{ const c: number[] = mkFake(22); T("S copyWithin fake", () => S_copyWithin(c), c); }
{ const h = new F_copyWithin(); h.k = mkFake(22); T("F copyWithin fake", () => h.run(), h.k); }
{ G = mkFake(22); T("G copyWithin fake", () => G_copyWithin(), G); }
{ const c: number[] = mkFake(22); T("L copyWithin fake", () => c.copyWithin(0, 1), c); }
{ const c: number[] = mkNone(22); T("P copyWithin none", () => P_copyWithin(c), c); }
{ const c: number[] = mkNone(22); T("S copyWithin none", () => S_copyWithin(c), c); }
{ const h = new F_copyWithin(); h.k = mkNone(22); T("F copyWithin none", () => h.run(), h.k); }
{ G = mkNone(22); T("G copyWithin none", () => G_copyWithin(), G); }
{ const c: number[] = mkNone(22); T("L copyWithin none", () => c.copyWithin(0, 1), c); }
{ const c: number[] = mkNamed(22); T("P copyWithin named", () => P_copyWithin(c), c); }
{ const c: number[] = mkNamed(22); T("S copyWithin named", () => S_copyWithin(c), c); }
{ const h = new F_copyWithin(); h.k = mkNamed(22); T("F copyWithin named", () => h.run(), h.k); }
{ G = mkNamed(22); T("G copyWithin named", () => G_copyWithin(), G); }
{ const c: number[] = mkNamed(22); T("L copyWithin named", () => c.copyWithin(0, 1), c); }
{ const c: number[] = mkLent(22); T("P copyWithin lent", () => P_copyWithin(c), c); }
{ const c: number[] = mkLent(22); T("S copyWithin lent", () => S_copyWithin(c), c); }
{ const h = new F_copyWithin(); h.k = mkLent(22); T("F copyWithin lent", () => h.run(), h.k); }
{ G = mkLent(22); T("G copyWithin lent", () => G_copyWithin(), G); }
{ const c: number[] = mkLent(22); T("L copyWithin lent", () => c.copyWithin(0, 1), c); }
{ const c: number[] = mk(); T("P copyWithin again", () => P_copyWithin(c), c); }
{ const c: number[] = mk(); T("S copyWithin again", () => S_copyWithin(c), c); }
{ const h = new F_copyWithin(); h.k = mk(); T("F copyWithin again", () => h.run(), h.k); }
{ G = mk(); T("G copyWithin again", () => G_copyWithin(), G); }
{ const c: number[] = mk(); T("L copyWithin again", () => c.copyWithin(0, 1), c); }
{ const c: number[] = mk(); T("P entries plain", () => P_entries(c), c); }
{ const c: number[] = mk(); T("S entries plain", () => S_entries(c), c); }
{ const h = new F_entries(); h.k = mk(); T("F entries plain", () => h.run(), h.k); }
{ G = mk(); T("G entries plain", () => G_entries(), G); }
{ const c: number[] = mk(); T("L entries plain", () => c.entries(), c); }
{ const c: number[] = mkOwn(23); T("P entries own", () => P_entries(c), c); }
{ const c: number[] = mkOwn(23); T("S entries own", () => S_entries(c), c); }
{ const h = new F_entries(); h.k = mkOwn(23); T("F entries own", () => h.run(), h.k); }
{ G = mkOwn(23); T("G entries own", () => G_entries(), G); }
{ const c: number[] = mkOwn(23); T("L entries own", () => c.entries(), c); }
{ const c: number[] = mkAcc(23); T("P entries acc", () => P_entries(c), c); }
{ const c: number[] = mkAcc(23); T("S entries acc", () => S_entries(c), c); }
{ const h = new F_entries(); h.k = mkAcc(23); T("F entries acc", () => h.run(), h.k); }
{ G = mkAcc(23); T("G entries acc", () => G_entries(), G); }
{ const c: number[] = mkAcc(23); T("L entries acc", () => c.entries(), c); }
{ const c: number[] = mkProto(23); T("P entries proto", () => P_entries(c), c); }
{ const c: number[] = mkProto(23); T("S entries proto", () => S_entries(c), c); }
{ const h = new F_entries(); h.k = mkProto(23); T("F entries proto", () => h.run(), h.k); }
{ G = mkProto(23); T("G entries proto", () => G_entries(), G); }
{ const c: number[] = mkProto(23); T("L entries proto", () => c.entries(), c); }
{ const c: number[] = mkSub(23); T("P entries sub", () => P_entries(c), c); }
{ const c: number[] = mkSub(23); T("S entries sub", () => S_entries(c), c); }
{ const h = new F_entries(); h.k = mkSub(23); T("F entries sub", () => h.run(), h.k); }
{ G = mkSub(23); T("G entries sub", () => G_entries(), G); }
{ const c: number[] = mkSub(23); T("L entries sub", () => c.entries(), c); }
{ const c: number[] = mkFake(23); T("P entries fake", () => P_entries(c), c); }
{ const c: number[] = mkFake(23); T("S entries fake", () => S_entries(c), c); }
{ const h = new F_entries(); h.k = mkFake(23); T("F entries fake", () => h.run(), h.k); }
{ G = mkFake(23); T("G entries fake", () => G_entries(), G); }
{ const c: number[] = mkFake(23); T("L entries fake", () => c.entries(), c); }
{ const c: number[] = mkNone(23); T("P entries none", () => P_entries(c), c); }
{ const c: number[] = mkNone(23); T("S entries none", () => S_entries(c), c); }
{ const h = new F_entries(); h.k = mkNone(23); T("F entries none", () => h.run(), h.k); }
{ G = mkNone(23); T("G entries none", () => G_entries(), G); }
{ const c: number[] = mkNone(23); T("L entries none", () => c.entries(), c); }
{ const c: number[] = mkNamed(23); T("P entries named", () => P_entries(c), c); }
{ const c: number[] = mkNamed(23); T("S entries named", () => S_entries(c), c); }
{ const h = new F_entries(); h.k = mkNamed(23); T("F entries named", () => h.run(), h.k); }
{ G = mkNamed(23); T("G entries named", () => G_entries(), G); }
{ const c: number[] = mkNamed(23); T("L entries named", () => c.entries(), c); }
{ const c: number[] = mkLent(23); T("P entries lent", () => P_entries(c), c); }
{ const c: number[] = mkLent(23); T("S entries lent", () => S_entries(c), c); }
{ const h = new F_entries(); h.k = mkLent(23); T("F entries lent", () => h.run(), h.k); }
{ G = mkLent(23); T("G entries lent", () => G_entries(), G); }
{ const c: number[] = mkLent(23); T("L entries lent", () => c.entries(), c); }
{ const c: number[] = mk(); T("P entries again", () => P_entries(c), c); }
{ const c: number[] = mk(); T("S entries again", () => S_entries(c), c); }
{ const h = new F_entries(); h.k = mk(); T("F entries again", () => h.run(), h.k); }
{ G = mk(); T("G entries again", () => G_entries(), G); }
{ const c: number[] = mk(); T("L entries again", () => c.entries(), c); }
{ const c: number[] = mk(); T("P findLast plain", () => P_findLast(c), c); }
{ const c: number[] = mk(); T("S findLast plain", () => S_findLast(c), c); }
{ const h = new F_findLast(); h.k = mk(); T("F findLast plain", () => h.run(), h.k); }
{ G = mk(); T("G findLast plain", () => G_findLast(), G); }
{ const c: number[] = mk(); T("L findLast plain", () => c.findLast((x: number) => x > 1), c); }
{ const c: number[] = mkOwn(24); T("P findLast own", () => P_findLast(c), c); }
{ const c: number[] = mkOwn(24); T("S findLast own", () => S_findLast(c), c); }
{ const h = new F_findLast(); h.k = mkOwn(24); T("F findLast own", () => h.run(), h.k); }
{ G = mkOwn(24); T("G findLast own", () => G_findLast(), G); }
{ const c: number[] = mkOwn(24); T("L findLast own", () => c.findLast((x: number) => x > 1), c); }
{ const c: number[] = mkAcc(24); T("P findLast acc", () => P_findLast(c), c); }
{ const c: number[] = mkAcc(24); T("S findLast acc", () => S_findLast(c), c); }
{ const h = new F_findLast(); h.k = mkAcc(24); T("F findLast acc", () => h.run(), h.k); }
{ G = mkAcc(24); T("G findLast acc", () => G_findLast(), G); }
{ const c: number[] = mkAcc(24); T("L findLast acc", () => c.findLast((x: number) => x > 1), c); }
{ const c: number[] = mkProto(24); T("P findLast proto", () => P_findLast(c), c); }
{ const c: number[] = mkProto(24); T("S findLast proto", () => S_findLast(c), c); }
{ const h = new F_findLast(); h.k = mkProto(24); T("F findLast proto", () => h.run(), h.k); }
{ G = mkProto(24); T("G findLast proto", () => G_findLast(), G); }
{ const c: number[] = mkProto(24); T("L findLast proto", () => c.findLast((x: number) => x > 1), c); }
{ const c: number[] = mkSub(24); T("P findLast sub", () => P_findLast(c), c); }
{ const c: number[] = mkSub(24); T("S findLast sub", () => S_findLast(c), c); }
{ const h = new F_findLast(); h.k = mkSub(24); T("F findLast sub", () => h.run(), h.k); }
{ G = mkSub(24); T("G findLast sub", () => G_findLast(), G); }
{ const c: number[] = mkSub(24); T("L findLast sub", () => c.findLast((x: number) => x > 1), c); }
{ const c: number[] = mkFake(24); T("P findLast fake", () => P_findLast(c), c); }
{ const c: number[] = mkFake(24); T("S findLast fake", () => S_findLast(c), c); }
{ const h = new F_findLast(); h.k = mkFake(24); T("F findLast fake", () => h.run(), h.k); }
{ G = mkFake(24); T("G findLast fake", () => G_findLast(), G); }
{ const c: number[] = mkFake(24); T("L findLast fake", () => c.findLast((x: number) => x > 1), c); }
{ const c: number[] = mkNone(24); T("P findLast none", () => P_findLast(c), c); }
{ const c: number[] = mkNone(24); T("S findLast none", () => S_findLast(c), c); }
{ const h = new F_findLast(); h.k = mkNone(24); T("F findLast none", () => h.run(), h.k); }
{ G = mkNone(24); T("G findLast none", () => G_findLast(), G); }
{ const c: number[] = mkNone(24); T("L findLast none", () => c.findLast((x: number) => x > 1), c); }
{ const c: number[] = mkNamed(24); T("P findLast named", () => P_findLast(c), c); }
{ const c: number[] = mkNamed(24); T("S findLast named", () => S_findLast(c), c); }
{ const h = new F_findLast(); h.k = mkNamed(24); T("F findLast named", () => h.run(), h.k); }
{ G = mkNamed(24); T("G findLast named", () => G_findLast(), G); }
{ const c: number[] = mkNamed(24); T("L findLast named", () => c.findLast((x: number) => x > 1), c); }
{ const c: number[] = mkLent(24); T("P findLast lent", () => P_findLast(c), c); }
{ const c: number[] = mkLent(24); T("S findLast lent", () => S_findLast(c), c); }
{ const h = new F_findLast(); h.k = mkLent(24); T("F findLast lent", () => h.run(), h.k); }
{ G = mkLent(24); T("G findLast lent", () => G_findLast(), G); }
{ const c: number[] = mkLent(24); T("L findLast lent", () => c.findLast((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P findLast again", () => P_findLast(c), c); }
{ const c: number[] = mk(); T("S findLast again", () => S_findLast(c), c); }
{ const h = new F_findLast(); h.k = mk(); T("F findLast again", () => h.run(), h.k); }
{ G = mk(); T("G findLast again", () => G_findLast(), G); }
{ const c: number[] = mk(); T("L findLast again", () => c.findLast((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P findLastIndex plain", () => P_findLastIndex(c), c); }
{ const c: number[] = mk(); T("S findLastIndex plain", () => S_findLastIndex(c), c); }
{ const h = new F_findLastIndex(); h.k = mk(); T("F findLastIndex plain", () => h.run(), h.k); }
{ G = mk(); T("G findLastIndex plain", () => G_findLastIndex(), G); }
{ const c: number[] = mk(); T("L findLastIndex plain", () => c.findLastIndex((x: number) => x > 1), c); }
{ const c: number[] = mkOwn(25); T("P findLastIndex own", () => P_findLastIndex(c), c); }
{ const c: number[] = mkOwn(25); T("S findLastIndex own", () => S_findLastIndex(c), c); }
{ const h = new F_findLastIndex(); h.k = mkOwn(25); T("F findLastIndex own", () => h.run(), h.k); }
{ G = mkOwn(25); T("G findLastIndex own", () => G_findLastIndex(), G); }
{ const c: number[] = mkOwn(25); T("L findLastIndex own", () => c.findLastIndex((x: number) => x > 1), c); }
{ const c: number[] = mkAcc(25); T("P findLastIndex acc", () => P_findLastIndex(c), c); }
{ const c: number[] = mkAcc(25); T("S findLastIndex acc", () => S_findLastIndex(c), c); }
{ const h = new F_findLastIndex(); h.k = mkAcc(25); T("F findLastIndex acc", () => h.run(), h.k); }
{ G = mkAcc(25); T("G findLastIndex acc", () => G_findLastIndex(), G); }
{ const c: number[] = mkAcc(25); T("L findLastIndex acc", () => c.findLastIndex((x: number) => x > 1), c); }
{ const c: number[] = mkProto(25); T("P findLastIndex proto", () => P_findLastIndex(c), c); }
{ const c: number[] = mkProto(25); T("S findLastIndex proto", () => S_findLastIndex(c), c); }
{ const h = new F_findLastIndex(); h.k = mkProto(25); T("F findLastIndex proto", () => h.run(), h.k); }
{ G = mkProto(25); T("G findLastIndex proto", () => G_findLastIndex(), G); }
{ const c: number[] = mkProto(25); T("L findLastIndex proto", () => c.findLastIndex((x: number) => x > 1), c); }
{ const c: number[] = mkSub(25); T("P findLastIndex sub", () => P_findLastIndex(c), c); }
{ const c: number[] = mkSub(25); T("S findLastIndex sub", () => S_findLastIndex(c), c); }
{ const h = new F_findLastIndex(); h.k = mkSub(25); T("F findLastIndex sub", () => h.run(), h.k); }
{ G = mkSub(25); T("G findLastIndex sub", () => G_findLastIndex(), G); }
{ const c: number[] = mkSub(25); T("L findLastIndex sub", () => c.findLastIndex((x: number) => x > 1), c); }
{ const c: number[] = mkFake(25); T("P findLastIndex fake", () => P_findLastIndex(c), c); }
{ const c: number[] = mkFake(25); T("S findLastIndex fake", () => S_findLastIndex(c), c); }
{ const h = new F_findLastIndex(); h.k = mkFake(25); T("F findLastIndex fake", () => h.run(), h.k); }
{ G = mkFake(25); T("G findLastIndex fake", () => G_findLastIndex(), G); }
{ const c: number[] = mkFake(25); T("L findLastIndex fake", () => c.findLastIndex((x: number) => x > 1), c); }
{ const c: number[] = mkNone(25); T("P findLastIndex none", () => P_findLastIndex(c), c); }
{ const c: number[] = mkNone(25); T("S findLastIndex none", () => S_findLastIndex(c), c); }
{ const h = new F_findLastIndex(); h.k = mkNone(25); T("F findLastIndex none", () => h.run(), h.k); }
{ G = mkNone(25); T("G findLastIndex none", () => G_findLastIndex(), G); }
{ const c: number[] = mkNone(25); T("L findLastIndex none", () => c.findLastIndex((x: number) => x > 1), c); }
{ const c: number[] = mkNamed(25); T("P findLastIndex named", () => P_findLastIndex(c), c); }
{ const c: number[] = mkNamed(25); T("S findLastIndex named", () => S_findLastIndex(c), c); }
{ const h = new F_findLastIndex(); h.k = mkNamed(25); T("F findLastIndex named", () => h.run(), h.k); }
{ G = mkNamed(25); T("G findLastIndex named", () => G_findLastIndex(), G); }
{ const c: number[] = mkNamed(25); T("L findLastIndex named", () => c.findLastIndex((x: number) => x > 1), c); }
{ const c: number[] = mkLent(25); T("P findLastIndex lent", () => P_findLastIndex(c), c); }
{ const c: number[] = mkLent(25); T("S findLastIndex lent", () => S_findLastIndex(c), c); }
{ const h = new F_findLastIndex(); h.k = mkLent(25); T("F findLastIndex lent", () => h.run(), h.k); }
{ G = mkLent(25); T("G findLastIndex lent", () => G_findLastIndex(), G); }
{ const c: number[] = mkLent(25); T("L findLastIndex lent", () => c.findLastIndex((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P findLastIndex again", () => P_findLastIndex(c), c); }
{ const c: number[] = mk(); T("S findLastIndex again", () => S_findLastIndex(c), c); }
{ const h = new F_findLastIndex(); h.k = mk(); T("F findLastIndex again", () => h.run(), h.k); }
{ G = mk(); T("G findLastIndex again", () => G_findLastIndex(), G); }
{ const c: number[] = mk(); T("L findLastIndex again", () => c.findLastIndex((x: number) => x > 1), c); }
{ const c: number[] = mk(); T("P flat plain", () => P_flat(c), c); }
{ const c: number[] = mk(); T("S flat plain", () => S_flat(c), c); }
{ const h = new F_flat(); h.k = mk(); T("F flat plain", () => h.run(), h.k); }
{ G = mk(); T("G flat plain", () => G_flat(), G); }
{ const c: number[] = mk(); T("L flat plain", () => c.flat(), c); }
{ const c: number[] = mkOwn(26); T("P flat own", () => P_flat(c), c); }
{ const c: number[] = mkOwn(26); T("S flat own", () => S_flat(c), c); }
{ const h = new F_flat(); h.k = mkOwn(26); T("F flat own", () => h.run(), h.k); }
{ G = mkOwn(26); T("G flat own", () => G_flat(), G); }
{ const c: number[] = mkOwn(26); T("L flat own", () => c.flat(), c); }
{ const c: number[] = mkAcc(26); T("P flat acc", () => P_flat(c), c); }
{ const c: number[] = mkAcc(26); T("S flat acc", () => S_flat(c), c); }
{ const h = new F_flat(); h.k = mkAcc(26); T("F flat acc", () => h.run(), h.k); }
{ G = mkAcc(26); T("G flat acc", () => G_flat(), G); }
{ const c: number[] = mkAcc(26); T("L flat acc", () => c.flat(), c); }
{ const c: number[] = mkProto(26); T("P flat proto", () => P_flat(c), c); }
{ const c: number[] = mkProto(26); T("S flat proto", () => S_flat(c), c); }
{ const h = new F_flat(); h.k = mkProto(26); T("F flat proto", () => h.run(), h.k); }
{ G = mkProto(26); T("G flat proto", () => G_flat(), G); }
{ const c: number[] = mkProto(26); T("L flat proto", () => c.flat(), c); }
{ const c: number[] = mkSub(26); T("P flat sub", () => P_flat(c), c); }
{ const c: number[] = mkSub(26); T("S flat sub", () => S_flat(c), c); }
{ const h = new F_flat(); h.k = mkSub(26); T("F flat sub", () => h.run(), h.k); }
{ G = mkSub(26); T("G flat sub", () => G_flat(), G); }
{ const c: number[] = mkSub(26); T("L flat sub", () => c.flat(), c); }
{ const c: number[] = mkFake(26); T("P flat fake", () => P_flat(c), c); }
{ const c: number[] = mkFake(26); T("S flat fake", () => S_flat(c), c); }
{ const h = new F_flat(); h.k = mkFake(26); T("F flat fake", () => h.run(), h.k); }
{ G = mkFake(26); T("G flat fake", () => G_flat(), G); }
{ const c: number[] = mkFake(26); T("L flat fake", () => c.flat(), c); }
{ const c: number[] = mkNone(26); T("P flat none", () => P_flat(c), c); }
{ const c: number[] = mkNone(26); T("S flat none", () => S_flat(c), c); }
{ const h = new F_flat(); h.k = mkNone(26); T("F flat none", () => h.run(), h.k); }
{ G = mkNone(26); T("G flat none", () => G_flat(), G); }
{ const c: number[] = mkNone(26); T("L flat none", () => c.flat(), c); }
{ const c: number[] = mkNamed(26); T("P flat named", () => P_flat(c), c); }
{ const c: number[] = mkNamed(26); T("S flat named", () => S_flat(c), c); }
{ const h = new F_flat(); h.k = mkNamed(26); T("F flat named", () => h.run(), h.k); }
{ G = mkNamed(26); T("G flat named", () => G_flat(), G); }
{ const c: number[] = mkNamed(26); T("L flat named", () => c.flat(), c); }
{ const c: number[] = mkLent(26); T("P flat lent", () => P_flat(c), c); }
{ const c: number[] = mkLent(26); T("S flat lent", () => S_flat(c), c); }
{ const h = new F_flat(); h.k = mkLent(26); T("F flat lent", () => h.run(), h.k); }
{ G = mkLent(26); T("G flat lent", () => G_flat(), G); }
{ const c: number[] = mkLent(26); T("L flat lent", () => c.flat(), c); }
{ const c: number[] = mk(); T("P flat again", () => P_flat(c), c); }
{ const c: number[] = mk(); T("S flat again", () => S_flat(c), c); }
{ const h = new F_flat(); h.k = mk(); T("F flat again", () => h.run(), h.k); }
{ G = mk(); T("G flat again", () => G_flat(), G); }
{ const c: number[] = mk(); T("L flat again", () => c.flat(), c); }
{ const c: number[] = mk(); T("P flatMap plain", () => P_flatMap(c), c); }
{ const c: number[] = mk(); T("S flatMap plain", () => S_flatMap(c), c); }
{ const h = new F_flatMap(); h.k = mk(); T("F flatMap plain", () => h.run(), h.k); }
{ G = mk(); T("G flatMap plain", () => G_flatMap(), G); }
{ const c: number[] = mk(); T("L flatMap plain", () => c.flatMap((x: number) => [x, x]), c); }
{ const c: number[] = mkOwn(27); T("P flatMap own", () => P_flatMap(c), c); }
{ const c: number[] = mkOwn(27); T("S flatMap own", () => S_flatMap(c), c); }
{ const h = new F_flatMap(); h.k = mkOwn(27); T("F flatMap own", () => h.run(), h.k); }
{ G = mkOwn(27); T("G flatMap own", () => G_flatMap(), G); }
{ const c: number[] = mkOwn(27); T("L flatMap own", () => c.flatMap((x: number) => [x, x]), c); }
{ const c: number[] = mkAcc(27); T("P flatMap acc", () => P_flatMap(c), c); }
{ const c: number[] = mkAcc(27); T("S flatMap acc", () => S_flatMap(c), c); }
{ const h = new F_flatMap(); h.k = mkAcc(27); T("F flatMap acc", () => h.run(), h.k); }
{ G = mkAcc(27); T("G flatMap acc", () => G_flatMap(), G); }
{ const c: number[] = mkAcc(27); T("L flatMap acc", () => c.flatMap((x: number) => [x, x]), c); }
{ const c: number[] = mkProto(27); T("P flatMap proto", () => P_flatMap(c), c); }
{ const c: number[] = mkProto(27); T("S flatMap proto", () => S_flatMap(c), c); }
{ const h = new F_flatMap(); h.k = mkProto(27); T("F flatMap proto", () => h.run(), h.k); }
{ G = mkProto(27); T("G flatMap proto", () => G_flatMap(), G); }
{ const c: number[] = mkProto(27); T("L flatMap proto", () => c.flatMap((x: number) => [x, x]), c); }
{ const c: number[] = mkSub(27); T("P flatMap sub", () => P_flatMap(c), c); }
{ const c: number[] = mkSub(27); T("S flatMap sub", () => S_flatMap(c), c); }
{ const h = new F_flatMap(); h.k = mkSub(27); T("F flatMap sub", () => h.run(), h.k); }
{ G = mkSub(27); T("G flatMap sub", () => G_flatMap(), G); }
{ const c: number[] = mkSub(27); T("L flatMap sub", () => c.flatMap((x: number) => [x, x]), c); }
{ const c: number[] = mkFake(27); T("P flatMap fake", () => P_flatMap(c), c); }
{ const c: number[] = mkFake(27); T("S flatMap fake", () => S_flatMap(c), c); }
{ const h = new F_flatMap(); h.k = mkFake(27); T("F flatMap fake", () => h.run(), h.k); }
{ G = mkFake(27); T("G flatMap fake", () => G_flatMap(), G); }
{ const c: number[] = mkFake(27); T("L flatMap fake", () => c.flatMap((x: number) => [x, x]), c); }
{ const c: number[] = mkNone(27); T("P flatMap none", () => P_flatMap(c), c); }
{ const c: number[] = mkNone(27); T("S flatMap none", () => S_flatMap(c), c); }
{ const h = new F_flatMap(); h.k = mkNone(27); T("F flatMap none", () => h.run(), h.k); }
{ G = mkNone(27); T("G flatMap none", () => G_flatMap(), G); }
{ const c: number[] = mkNone(27); T("L flatMap none", () => c.flatMap((x: number) => [x, x]), c); }
{ const c: number[] = mkNamed(27); T("P flatMap named", () => P_flatMap(c), c); }
{ const c: number[] = mkNamed(27); T("S flatMap named", () => S_flatMap(c), c); }
{ const h = new F_flatMap(); h.k = mkNamed(27); T("F flatMap named", () => h.run(), h.k); }
{ G = mkNamed(27); T("G flatMap named", () => G_flatMap(), G); }
{ const c: number[] = mkNamed(27); T("L flatMap named", () => c.flatMap((x: number) => [x, x]), c); }
{ const c: number[] = mkLent(27); T("P flatMap lent", () => P_flatMap(c), c); }
{ const c: number[] = mkLent(27); T("S flatMap lent", () => S_flatMap(c), c); }
{ const h = new F_flatMap(); h.k = mkLent(27); T("F flatMap lent", () => h.run(), h.k); }
{ G = mkLent(27); T("G flatMap lent", () => G_flatMap(), G); }
{ const c: number[] = mkLent(27); T("L flatMap lent", () => c.flatMap((x: number) => [x, x]), c); }
{ const c: number[] = mk(); T("P flatMap again", () => P_flatMap(c), c); }
{ const c: number[] = mk(); T("S flatMap again", () => S_flatMap(c), c); }
{ const h = new F_flatMap(); h.k = mk(); T("F flatMap again", () => h.run(), h.k); }
{ G = mk(); T("G flatMap again", () => G_flatMap(), G); }
{ const c: number[] = mk(); T("L flatMap again", () => c.flatMap((x: number) => [x, x]), c); }
{ const c: number[] = mk(); T("P keys plain", () => P_keys(c), c); }
{ const c: number[] = mk(); T("S keys plain", () => S_keys(c), c); }
{ const h = new F_keys(); h.k = mk(); T("F keys plain", () => h.run(), h.k); }
{ G = mk(); T("G keys plain", () => G_keys(), G); }
{ const c: number[] = mk(); T("L keys plain", () => c.keys(), c); }
{ const c: number[] = mkOwn(28); T("P keys own", () => P_keys(c), c); }
{ const c: number[] = mkOwn(28); T("S keys own", () => S_keys(c), c); }
{ const h = new F_keys(); h.k = mkOwn(28); T("F keys own", () => h.run(), h.k); }
{ G = mkOwn(28); T("G keys own", () => G_keys(), G); }
{ const c: number[] = mkOwn(28); T("L keys own", () => c.keys(), c); }
{ const c: number[] = mkAcc(28); T("P keys acc", () => P_keys(c), c); }
{ const c: number[] = mkAcc(28); T("S keys acc", () => S_keys(c), c); }
{ const h = new F_keys(); h.k = mkAcc(28); T("F keys acc", () => h.run(), h.k); }
{ G = mkAcc(28); T("G keys acc", () => G_keys(), G); }
{ const c: number[] = mkAcc(28); T("L keys acc", () => c.keys(), c); }
{ const c: number[] = mkProto(28); T("P keys proto", () => P_keys(c), c); }
{ const c: number[] = mkProto(28); T("S keys proto", () => S_keys(c), c); }
{ const h = new F_keys(); h.k = mkProto(28); T("F keys proto", () => h.run(), h.k); }
{ G = mkProto(28); T("G keys proto", () => G_keys(), G); }
{ const c: number[] = mkProto(28); T("L keys proto", () => c.keys(), c); }
{ const c: number[] = mkSub(28); T("P keys sub", () => P_keys(c), c); }
{ const c: number[] = mkSub(28); T("S keys sub", () => S_keys(c), c); }
{ const h = new F_keys(); h.k = mkSub(28); T("F keys sub", () => h.run(), h.k); }
{ G = mkSub(28); T("G keys sub", () => G_keys(), G); }
{ const c: number[] = mkSub(28); T("L keys sub", () => c.keys(), c); }
{ const c: number[] = mkFake(28); T("P keys fake", () => P_keys(c), c); }
{ const c: number[] = mkFake(28); T("S keys fake", () => S_keys(c), c); }
{ const h = new F_keys(); h.k = mkFake(28); T("F keys fake", () => h.run(), h.k); }
{ G = mkFake(28); T("G keys fake", () => G_keys(), G); }
{ const c: number[] = mkFake(28); T("L keys fake", () => c.keys(), c); }
{ const c: number[] = mkNone(28); T("P keys none", () => P_keys(c), c); }
{ const c: number[] = mkNone(28); T("S keys none", () => S_keys(c), c); }
{ const h = new F_keys(); h.k = mkNone(28); T("F keys none", () => h.run(), h.k); }
{ G = mkNone(28); T("G keys none", () => G_keys(), G); }
{ const c: number[] = mkNone(28); T("L keys none", () => c.keys(), c); }
{ const c: number[] = mkNamed(28); T("P keys named", () => P_keys(c), c); }
{ const c: number[] = mkNamed(28); T("S keys named", () => S_keys(c), c); }
{ const h = new F_keys(); h.k = mkNamed(28); T("F keys named", () => h.run(), h.k); }
{ G = mkNamed(28); T("G keys named", () => G_keys(), G); }
{ const c: number[] = mkNamed(28); T("L keys named", () => c.keys(), c); }
{ const c: number[] = mkLent(28); T("P keys lent", () => P_keys(c), c); }
{ const c: number[] = mkLent(28); T("S keys lent", () => S_keys(c), c); }
{ const h = new F_keys(); h.k = mkLent(28); T("F keys lent", () => h.run(), h.k); }
{ G = mkLent(28); T("G keys lent", () => G_keys(), G); }
{ const c: number[] = mkLent(28); T("L keys lent", () => c.keys(), c); }
{ const c: number[] = mk(); T("P keys again", () => P_keys(c), c); }
{ const c: number[] = mk(); T("S keys again", () => S_keys(c), c); }
{ const h = new F_keys(); h.k = mk(); T("F keys again", () => h.run(), h.k); }
{ G = mk(); T("G keys again", () => G_keys(), G); }
{ const c: number[] = mk(); T("L keys again", () => c.keys(), c); }
{ const c: number[] = mk(); T("P reduceRight plain", () => P_reduceRight(c), c); }
{ const c: number[] = mk(); T("S reduceRight plain", () => S_reduceRight(c), c); }
{ const h = new F_reduceRight(); h.k = mk(); T("F reduceRight plain", () => h.run(), h.k); }
{ G = mk(); T("G reduceRight plain", () => G_reduceRight(), G); }
{ const c: number[] = mk(); T("L reduceRight plain", () => c.reduceRight((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkOwn(29); T("P reduceRight own", () => P_reduceRight(c), c); }
{ const c: number[] = mkOwn(29); T("S reduceRight own", () => S_reduceRight(c), c); }
{ const h = new F_reduceRight(); h.k = mkOwn(29); T("F reduceRight own", () => h.run(), h.k); }
{ G = mkOwn(29); T("G reduceRight own", () => G_reduceRight(), G); }
{ const c: number[] = mkOwn(29); T("L reduceRight own", () => c.reduceRight((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkAcc(29); T("P reduceRight acc", () => P_reduceRight(c), c); }
{ const c: number[] = mkAcc(29); T("S reduceRight acc", () => S_reduceRight(c), c); }
{ const h = new F_reduceRight(); h.k = mkAcc(29); T("F reduceRight acc", () => h.run(), h.k); }
{ G = mkAcc(29); T("G reduceRight acc", () => G_reduceRight(), G); }
{ const c: number[] = mkAcc(29); T("L reduceRight acc", () => c.reduceRight((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkProto(29); T("P reduceRight proto", () => P_reduceRight(c), c); }
{ const c: number[] = mkProto(29); T("S reduceRight proto", () => S_reduceRight(c), c); }
{ const h = new F_reduceRight(); h.k = mkProto(29); T("F reduceRight proto", () => h.run(), h.k); }
{ G = mkProto(29); T("G reduceRight proto", () => G_reduceRight(), G); }
{ const c: number[] = mkProto(29); T("L reduceRight proto", () => c.reduceRight((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkSub(29); T("P reduceRight sub", () => P_reduceRight(c), c); }
{ const c: number[] = mkSub(29); T("S reduceRight sub", () => S_reduceRight(c), c); }
{ const h = new F_reduceRight(); h.k = mkSub(29); T("F reduceRight sub", () => h.run(), h.k); }
{ G = mkSub(29); T("G reduceRight sub", () => G_reduceRight(), G); }
{ const c: number[] = mkSub(29); T("L reduceRight sub", () => c.reduceRight((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkFake(29); T("P reduceRight fake", () => P_reduceRight(c), c); }
{ const c: number[] = mkFake(29); T("S reduceRight fake", () => S_reduceRight(c), c); }
{ const h = new F_reduceRight(); h.k = mkFake(29); T("F reduceRight fake", () => h.run(), h.k); }
{ G = mkFake(29); T("G reduceRight fake", () => G_reduceRight(), G); }
{ const c: number[] = mkFake(29); T("L reduceRight fake", () => c.reduceRight((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkNone(29); T("P reduceRight none", () => P_reduceRight(c), c); }
{ const c: number[] = mkNone(29); T("S reduceRight none", () => S_reduceRight(c), c); }
{ const h = new F_reduceRight(); h.k = mkNone(29); T("F reduceRight none", () => h.run(), h.k); }
{ G = mkNone(29); T("G reduceRight none", () => G_reduceRight(), G); }
{ const c: number[] = mkNone(29); T("L reduceRight none", () => c.reduceRight((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkNamed(29); T("P reduceRight named", () => P_reduceRight(c), c); }
{ const c: number[] = mkNamed(29); T("S reduceRight named", () => S_reduceRight(c), c); }
{ const h = new F_reduceRight(); h.k = mkNamed(29); T("F reduceRight named", () => h.run(), h.k); }
{ G = mkNamed(29); T("G reduceRight named", () => G_reduceRight(), G); }
{ const c: number[] = mkNamed(29); T("L reduceRight named", () => c.reduceRight((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mkLent(29); T("P reduceRight lent", () => P_reduceRight(c), c); }
{ const c: number[] = mkLent(29); T("S reduceRight lent", () => S_reduceRight(c), c); }
{ const h = new F_reduceRight(); h.k = mkLent(29); T("F reduceRight lent", () => h.run(), h.k); }
{ G = mkLent(29); T("G reduceRight lent", () => G_reduceRight(), G); }
{ const c: number[] = mkLent(29); T("L reduceRight lent", () => c.reduceRight((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mk(); T("P reduceRight again", () => P_reduceRight(c), c); }
{ const c: number[] = mk(); T("S reduceRight again", () => S_reduceRight(c), c); }
{ const h = new F_reduceRight(); h.k = mk(); T("F reduceRight again", () => h.run(), h.k); }
{ G = mk(); T("G reduceRight again", () => G_reduceRight(), G); }
{ const c: number[] = mk(); T("L reduceRight again", () => c.reduceRight((p: number, x: number) => p + x, 0), c); }
{ const c: number[] = mk(); T("P toReversed plain", () => P_toReversed(c), c); }
{ const c: number[] = mk(); T("S toReversed plain", () => S_toReversed(c), c); }
{ const h = new F_toReversed(); h.k = mk(); T("F toReversed plain", () => h.run(), h.k); }
{ G = mk(); T("G toReversed plain", () => G_toReversed(), G); }
{ const c: number[] = mk(); T("L toReversed plain", () => c.toReversed(), c); }
{ const c: number[] = mkOwn(30); T("P toReversed own", () => P_toReversed(c), c); }
{ const c: number[] = mkOwn(30); T("S toReversed own", () => S_toReversed(c), c); }
{ const h = new F_toReversed(); h.k = mkOwn(30); T("F toReversed own", () => h.run(), h.k); }
{ G = mkOwn(30); T("G toReversed own", () => G_toReversed(), G); }
{ const c: number[] = mkOwn(30); T("L toReversed own", () => c.toReversed(), c); }
{ const c: number[] = mkAcc(30); T("P toReversed acc", () => P_toReversed(c), c); }
{ const c: number[] = mkAcc(30); T("S toReversed acc", () => S_toReversed(c), c); }
{ const h = new F_toReversed(); h.k = mkAcc(30); T("F toReversed acc", () => h.run(), h.k); }
{ G = mkAcc(30); T("G toReversed acc", () => G_toReversed(), G); }
{ const c: number[] = mkAcc(30); T("L toReversed acc", () => c.toReversed(), c); }
{ const c: number[] = mkProto(30); T("P toReversed proto", () => P_toReversed(c), c); }
{ const c: number[] = mkProto(30); T("S toReversed proto", () => S_toReversed(c), c); }
{ const h = new F_toReversed(); h.k = mkProto(30); T("F toReversed proto", () => h.run(), h.k); }
{ G = mkProto(30); T("G toReversed proto", () => G_toReversed(), G); }
{ const c: number[] = mkProto(30); T("L toReversed proto", () => c.toReversed(), c); }
{ const c: number[] = mkSub(30); T("P toReversed sub", () => P_toReversed(c), c); }
{ const c: number[] = mkSub(30); T("S toReversed sub", () => S_toReversed(c), c); }
{ const h = new F_toReversed(); h.k = mkSub(30); T("F toReversed sub", () => h.run(), h.k); }
{ G = mkSub(30); T("G toReversed sub", () => G_toReversed(), G); }
{ const c: number[] = mkSub(30); T("L toReversed sub", () => c.toReversed(), c); }
{ const c: number[] = mkFake(30); T("P toReversed fake", () => P_toReversed(c), c); }
{ const c: number[] = mkFake(30); T("S toReversed fake", () => S_toReversed(c), c); }
{ const h = new F_toReversed(); h.k = mkFake(30); T("F toReversed fake", () => h.run(), h.k); }
{ G = mkFake(30); T("G toReversed fake", () => G_toReversed(), G); }
{ const c: number[] = mkFake(30); T("L toReversed fake", () => c.toReversed(), c); }
{ const c: number[] = mkNone(30); T("P toReversed none", () => P_toReversed(c), c); }
{ const c: number[] = mkNone(30); T("S toReversed none", () => S_toReversed(c), c); }
{ const h = new F_toReversed(); h.k = mkNone(30); T("F toReversed none", () => h.run(), h.k); }
{ G = mkNone(30); T("G toReversed none", () => G_toReversed(), G); }
{ const c: number[] = mkNone(30); T("L toReversed none", () => c.toReversed(), c); }
{ const c: number[] = mkNamed(30); T("P toReversed named", () => P_toReversed(c), c); }
{ const c: number[] = mkNamed(30); T("S toReversed named", () => S_toReversed(c), c); }
{ const h = new F_toReversed(); h.k = mkNamed(30); T("F toReversed named", () => h.run(), h.k); }
{ G = mkNamed(30); T("G toReversed named", () => G_toReversed(), G); }
{ const c: number[] = mkNamed(30); T("L toReversed named", () => c.toReversed(), c); }
{ const c: number[] = mkLent(30); T("P toReversed lent", () => P_toReversed(c), c); }
{ const c: number[] = mkLent(30); T("S toReversed lent", () => S_toReversed(c), c); }
{ const h = new F_toReversed(); h.k = mkLent(30); T("F toReversed lent", () => h.run(), h.k); }
{ G = mkLent(30); T("G toReversed lent", () => G_toReversed(), G); }
{ const c: number[] = mkLent(30); T("L toReversed lent", () => c.toReversed(), c); }
{ const c: number[] = mk(); T("P toReversed again", () => P_toReversed(c), c); }
{ const c: number[] = mk(); T("S toReversed again", () => S_toReversed(c), c); }
{ const h = new F_toReversed(); h.k = mk(); T("F toReversed again", () => h.run(), h.k); }
{ G = mk(); T("G toReversed again", () => G_toReversed(), G); }
{ const c: number[] = mk(); T("L toReversed again", () => c.toReversed(), c); }
{ const c: number[] = mk(); T("P toSorted plain", () => P_toSorted(c), c); }
{ const c: number[] = mk(); T("S toSorted plain", () => S_toSorted(c), c); }
{ const h = new F_toSorted(); h.k = mk(); T("F toSorted plain", () => h.run(), h.k); }
{ G = mk(); T("G toSorted plain", () => G_toSorted(), G); }
{ const c: number[] = mk(); T("L toSorted plain", () => c.toSorted(), c); }
{ const c: number[] = mkOwn(31); T("P toSorted own", () => P_toSorted(c), c); }
{ const c: number[] = mkOwn(31); T("S toSorted own", () => S_toSorted(c), c); }
{ const h = new F_toSorted(); h.k = mkOwn(31); T("F toSorted own", () => h.run(), h.k); }
{ G = mkOwn(31); T("G toSorted own", () => G_toSorted(), G); }
{ const c: number[] = mkOwn(31); T("L toSorted own", () => c.toSorted(), c); }
{ const c: number[] = mkAcc(31); T("P toSorted acc", () => P_toSorted(c), c); }
{ const c: number[] = mkAcc(31); T("S toSorted acc", () => S_toSorted(c), c); }
{ const h = new F_toSorted(); h.k = mkAcc(31); T("F toSorted acc", () => h.run(), h.k); }
{ G = mkAcc(31); T("G toSorted acc", () => G_toSorted(), G); }
{ const c: number[] = mkAcc(31); T("L toSorted acc", () => c.toSorted(), c); }
{ const c: number[] = mkProto(31); T("P toSorted proto", () => P_toSorted(c), c); }
{ const c: number[] = mkProto(31); T("S toSorted proto", () => S_toSorted(c), c); }
{ const h = new F_toSorted(); h.k = mkProto(31); T("F toSorted proto", () => h.run(), h.k); }
{ G = mkProto(31); T("G toSorted proto", () => G_toSorted(), G); }
{ const c: number[] = mkProto(31); T("L toSorted proto", () => c.toSorted(), c); }
{ const c: number[] = mkSub(31); T("P toSorted sub", () => P_toSorted(c), c); }
{ const c: number[] = mkSub(31); T("S toSorted sub", () => S_toSorted(c), c); }
{ const h = new F_toSorted(); h.k = mkSub(31); T("F toSorted sub", () => h.run(), h.k); }
{ G = mkSub(31); T("G toSorted sub", () => G_toSorted(), G); }
{ const c: number[] = mkSub(31); T("L toSorted sub", () => c.toSorted(), c); }
{ const c: number[] = mkFake(31); T("P toSorted fake", () => P_toSorted(c), c); }
{ const c: number[] = mkFake(31); T("S toSorted fake", () => S_toSorted(c), c); }
{ const h = new F_toSorted(); h.k = mkFake(31); T("F toSorted fake", () => h.run(), h.k); }
{ G = mkFake(31); T("G toSorted fake", () => G_toSorted(), G); }
{ const c: number[] = mkFake(31); T("L toSorted fake", () => c.toSorted(), c); }
{ const c: number[] = mkNone(31); T("P toSorted none", () => P_toSorted(c), c); }
{ const c: number[] = mkNone(31); T("S toSorted none", () => S_toSorted(c), c); }
{ const h = new F_toSorted(); h.k = mkNone(31); T("F toSorted none", () => h.run(), h.k); }
{ G = mkNone(31); T("G toSorted none", () => G_toSorted(), G); }
{ const c: number[] = mkNone(31); T("L toSorted none", () => c.toSorted(), c); }
{ const c: number[] = mkNamed(31); T("P toSorted named", () => P_toSorted(c), c); }
{ const c: number[] = mkNamed(31); T("S toSorted named", () => S_toSorted(c), c); }
{ const h = new F_toSorted(); h.k = mkNamed(31); T("F toSorted named", () => h.run(), h.k); }
{ G = mkNamed(31); T("G toSorted named", () => G_toSorted(), G); }
{ const c: number[] = mkNamed(31); T("L toSorted named", () => c.toSorted(), c); }
{ const c: number[] = mkLent(31); T("P toSorted lent", () => P_toSorted(c), c); }
{ const c: number[] = mkLent(31); T("S toSorted lent", () => S_toSorted(c), c); }
{ const h = new F_toSorted(); h.k = mkLent(31); T("F toSorted lent", () => h.run(), h.k); }
{ G = mkLent(31); T("G toSorted lent", () => G_toSorted(), G); }
{ const c: number[] = mkLent(31); T("L toSorted lent", () => c.toSorted(), c); }
{ const c: number[] = mk(); T("P toSorted again", () => P_toSorted(c), c); }
{ const c: number[] = mk(); T("S toSorted again", () => S_toSorted(c), c); }
{ const h = new F_toSorted(); h.k = mk(); T("F toSorted again", () => h.run(), h.k); }
{ G = mk(); T("G toSorted again", () => G_toSorted(), G); }
{ const c: number[] = mk(); T("L toSorted again", () => c.toSorted(), c); }
{ const c: number[] = mk(); T("P toSpliced plain", () => P_toSpliced(c), c); }
{ const c: number[] = mk(); T("S toSpliced plain", () => S_toSpliced(c), c); }
{ const h = new F_toSpliced(); h.k = mk(); T("F toSpliced plain", () => h.run(), h.k); }
{ G = mk(); T("G toSpliced plain", () => G_toSpliced(), G); }
{ const c: number[] = mk(); T("L toSpliced plain", () => c.toSpliced(0, 1), c); }
{ const c: number[] = mkOwn(32); T("P toSpliced own", () => P_toSpliced(c), c); }
{ const c: number[] = mkOwn(32); T("S toSpliced own", () => S_toSpliced(c), c); }
{ const h = new F_toSpliced(); h.k = mkOwn(32); T("F toSpliced own", () => h.run(), h.k); }
{ G = mkOwn(32); T("G toSpliced own", () => G_toSpliced(), G); }
{ const c: number[] = mkOwn(32); T("L toSpliced own", () => c.toSpliced(0, 1), c); }
{ const c: number[] = mkAcc(32); T("P toSpliced acc", () => P_toSpliced(c), c); }
{ const c: number[] = mkAcc(32); T("S toSpliced acc", () => S_toSpliced(c), c); }
{ const h = new F_toSpliced(); h.k = mkAcc(32); T("F toSpliced acc", () => h.run(), h.k); }
{ G = mkAcc(32); T("G toSpliced acc", () => G_toSpliced(), G); }
{ const c: number[] = mkAcc(32); T("L toSpliced acc", () => c.toSpliced(0, 1), c); }
{ const c: number[] = mkProto(32); T("P toSpliced proto", () => P_toSpliced(c), c); }
{ const c: number[] = mkProto(32); T("S toSpliced proto", () => S_toSpliced(c), c); }
{ const h = new F_toSpliced(); h.k = mkProto(32); T("F toSpliced proto", () => h.run(), h.k); }
{ G = mkProto(32); T("G toSpliced proto", () => G_toSpliced(), G); }
{ const c: number[] = mkProto(32); T("L toSpliced proto", () => c.toSpliced(0, 1), c); }
{ const c: number[] = mkSub(32); T("P toSpliced sub", () => P_toSpliced(c), c); }
{ const c: number[] = mkSub(32); T("S toSpliced sub", () => S_toSpliced(c), c); }
{ const h = new F_toSpliced(); h.k = mkSub(32); T("F toSpliced sub", () => h.run(), h.k); }
{ G = mkSub(32); T("G toSpliced sub", () => G_toSpliced(), G); }
{ const c: number[] = mkSub(32); T("L toSpliced sub", () => c.toSpliced(0, 1), c); }
{ const c: number[] = mkFake(32); T("P toSpliced fake", () => P_toSpliced(c), c); }
{ const c: number[] = mkFake(32); T("S toSpliced fake", () => S_toSpliced(c), c); }
{ const h = new F_toSpliced(); h.k = mkFake(32); T("F toSpliced fake", () => h.run(), h.k); }
{ G = mkFake(32); T("G toSpliced fake", () => G_toSpliced(), G); }
{ const c: number[] = mkFake(32); T("L toSpliced fake", () => c.toSpliced(0, 1), c); }
{ const c: number[] = mkNone(32); T("P toSpliced none", () => P_toSpliced(c), c); }
{ const c: number[] = mkNone(32); T("S toSpliced none", () => S_toSpliced(c), c); }
{ const h = new F_toSpliced(); h.k = mkNone(32); T("F toSpliced none", () => h.run(), h.k); }
{ G = mkNone(32); T("G toSpliced none", () => G_toSpliced(), G); }
{ const c: number[] = mkNone(32); T("L toSpliced none", () => c.toSpliced(0, 1), c); }
{ const c: number[] = mkNamed(32); T("P toSpliced named", () => P_toSpliced(c), c); }
{ const c: number[] = mkNamed(32); T("S toSpliced named", () => S_toSpliced(c), c); }
{ const h = new F_toSpliced(); h.k = mkNamed(32); T("F toSpliced named", () => h.run(), h.k); }
{ G = mkNamed(32); T("G toSpliced named", () => G_toSpliced(), G); }
{ const c: number[] = mkNamed(32); T("L toSpliced named", () => c.toSpliced(0, 1), c); }
{ const c: number[] = mk(); T("P toSpliced again", () => P_toSpliced(c), c); }
{ const c: number[] = mk(); T("S toSpliced again", () => S_toSpliced(c), c); }
{ const h = new F_toSpliced(); h.k = mk(); T("F toSpliced again", () => h.run(), h.k); }
{ G = mk(); T("G toSpliced again", () => G_toSpliced(), G); }
{ const c: number[] = mk(); T("L toSpliced again", () => c.toSpliced(0, 1), c); }
{ const c: number[] = mk(); T("P values plain", () => P_values(c), c); }
{ const c: number[] = mk(); T("S values plain", () => S_values(c), c); }
{ const h = new F_values(); h.k = mk(); T("F values plain", () => h.run(), h.k); }
{ G = mk(); T("G values plain", () => G_values(), G); }
{ const c: number[] = mk(); T("L values plain", () => c.values(), c); }
{ const c: number[] = mkOwn(33); T("P values own", () => P_values(c), c); }
{ const c: number[] = mkOwn(33); T("S values own", () => S_values(c), c); }
{ const h = new F_values(); h.k = mkOwn(33); T("F values own", () => h.run(), h.k); }
{ G = mkOwn(33); T("G values own", () => G_values(), G); }
{ const c: number[] = mkOwn(33); T("L values own", () => c.values(), c); }
{ const c: number[] = mkAcc(33); T("P values acc", () => P_values(c), c); }
{ const c: number[] = mkAcc(33); T("S values acc", () => S_values(c), c); }
{ const h = new F_values(); h.k = mkAcc(33); T("F values acc", () => h.run(), h.k); }
{ G = mkAcc(33); T("G values acc", () => G_values(), G); }
{ const c: number[] = mkAcc(33); T("L values acc", () => c.values(), c); }
{ const c: number[] = mkProto(33); T("P values proto", () => P_values(c), c); }
{ const c: number[] = mkProto(33); T("S values proto", () => S_values(c), c); }
{ const h = new F_values(); h.k = mkProto(33); T("F values proto", () => h.run(), h.k); }
{ G = mkProto(33); T("G values proto", () => G_values(), G); }
{ const c: number[] = mkProto(33); T("L values proto", () => c.values(), c); }
{ const c: number[] = mkSub(33); T("P values sub", () => P_values(c), c); }
{ const c: number[] = mkSub(33); T("S values sub", () => S_values(c), c); }
{ const h = new F_values(); h.k = mkSub(33); T("F values sub", () => h.run(), h.k); }
{ G = mkSub(33); T("G values sub", () => G_values(), G); }
{ const c: number[] = mkSub(33); T("L values sub", () => c.values(), c); }
{ const c: number[] = mkFake(33); T("P values fake", () => P_values(c), c); }
{ const c: number[] = mkFake(33); T("S values fake", () => S_values(c), c); }
{ const h = new F_values(); h.k = mkFake(33); T("F values fake", () => h.run(), h.k); }
{ G = mkFake(33); T("G values fake", () => G_values(), G); }
{ const c: number[] = mkFake(33); T("L values fake", () => c.values(), c); }
{ const c: number[] = mkNone(33); T("P values none", () => P_values(c), c); }
{ const c: number[] = mkNone(33); T("S values none", () => S_values(c), c); }
{ const h = new F_values(); h.k = mkNone(33); T("F values none", () => h.run(), h.k); }
{ G = mkNone(33); T("G values none", () => G_values(), G); }
{ const c: number[] = mkNone(33); T("L values none", () => c.values(), c); }
{ const c: number[] = mkNamed(33); T("P values named", () => P_values(c), c); }
{ const c: number[] = mkNamed(33); T("S values named", () => S_values(c), c); }
{ const h = new F_values(); h.k = mkNamed(33); T("F values named", () => h.run(), h.k); }
{ G = mkNamed(33); T("G values named", () => G_values(), G); }
{ const c: number[] = mkNamed(33); T("L values named", () => c.values(), c); }
{ const c: number[] = mkLent(33); T("P values lent", () => P_values(c), c); }
{ const c: number[] = mkLent(33); T("S values lent", () => S_values(c), c); }
{ const h = new F_values(); h.k = mkLent(33); T("F values lent", () => h.run(), h.k); }
{ G = mkLent(33); T("G values lent", () => G_values(), G); }
{ const c: number[] = mkLent(33); T("L values lent", () => c.values(), c); }
{ const c: number[] = mk(); T("P values again", () => P_values(c), c); }
{ const c: number[] = mk(); T("S values again", () => S_values(c), c); }
{ const h = new F_values(); h.k = mk(); T("F values again", () => h.run(), h.k); }
{ G = mk(); T("G values again", () => G_values(), G); }
{ const c: number[] = mk(); T("L values again", () => c.values(), c); }
{ const c: number[] = mk(); T("P with plain", () => P_with(c), c); }
{ const c: number[] = mk(); T("S with plain", () => S_with(c), c); }
{ const h = new F_with(); h.k = mk(); T("F with plain", () => h.run(), h.k); }
{ G = mk(); T("G with plain", () => G_with(), G); }
{ const c: number[] = mk(); T("L with plain", () => c.with(0, 9), c); }
{ const c: number[] = mkOwn(34); T("P with own", () => P_with(c), c); }
{ const c: number[] = mkOwn(34); T("S with own", () => S_with(c), c); }
{ const h = new F_with(); h.k = mkOwn(34); T("F with own", () => h.run(), h.k); }
{ G = mkOwn(34); T("G with own", () => G_with(), G); }
{ const c: number[] = mkOwn(34); T("L with own", () => c.with(0, 9), c); }
{ const c: number[] = mkAcc(34); T("P with acc", () => P_with(c), c); }
{ const c: number[] = mkAcc(34); T("S with acc", () => S_with(c), c); }
{ const h = new F_with(); h.k = mkAcc(34); T("F with acc", () => h.run(), h.k); }
{ G = mkAcc(34); T("G with acc", () => G_with(), G); }
{ const c: number[] = mkAcc(34); T("L with acc", () => c.with(0, 9), c); }
{ const c: number[] = mkProto(34); T("P with proto", () => P_with(c), c); }
{ const c: number[] = mkProto(34); T("S with proto", () => S_with(c), c); }
{ const h = new F_with(); h.k = mkProto(34); T("F with proto", () => h.run(), h.k); }
{ G = mkProto(34); T("G with proto", () => G_with(), G); }
{ const c: number[] = mkProto(34); T("L with proto", () => c.with(0, 9), c); }
{ const c: number[] = mkSub(34); T("P with sub", () => P_with(c), c); }
{ const c: number[] = mkSub(34); T("S with sub", () => S_with(c), c); }
{ const h = new F_with(); h.k = mkSub(34); T("F with sub", () => h.run(), h.k); }
{ G = mkSub(34); T("G with sub", () => G_with(), G); }
{ const c: number[] = mkSub(34); T("L with sub", () => c.with(0, 9), c); }
{ const c: number[] = mkFake(34); T("P with fake", () => P_with(c), c); }
{ const c: number[] = mkFake(34); T("S with fake", () => S_with(c), c); }
{ const h = new F_with(); h.k = mkFake(34); T("F with fake", () => h.run(), h.k); }
{ G = mkFake(34); T("G with fake", () => G_with(), G); }
{ const c: number[] = mkFake(34); T("L with fake", () => c.with(0, 9), c); }
{ const c: number[] = mkNone(34); T("P with none", () => P_with(c), c); }
{ const c: number[] = mkNone(34); T("S with none", () => S_with(c), c); }
{ const h = new F_with(); h.k = mkNone(34); T("F with none", () => h.run(), h.k); }
{ G = mkNone(34); T("G with none", () => G_with(), G); }
{ const c: number[] = mkNone(34); T("L with none", () => c.with(0, 9), c); }
{ const c: number[] = mkNamed(34); T("P with named", () => P_with(c), c); }
{ const c: number[] = mkNamed(34); T("S with named", () => S_with(c), c); }
{ const h = new F_with(); h.k = mkNamed(34); T("F with named", () => h.run(), h.k); }
{ G = mkNamed(34); T("G with named", () => G_with(), G); }
{ const c: number[] = mkNamed(34); T("L with named", () => c.with(0, 9), c); }
{ const c: number[] = mkLent(34); T("P with lent", () => P_with(c), c); }
{ const c: number[] = mkLent(34); T("S with lent", () => S_with(c), c); }
{ const h = new F_with(); h.k = mkLent(34); T("F with lent", () => h.run(), h.k); }
{ G = mkLent(34); T("G with lent", () => G_with(), G); }
{ const c: number[] = mkLent(34); T("L with lent", () => c.with(0, 9), c); }
{ const c: number[] = mk(); T("P with again", () => P_with(c), c); }
{ const c: number[] = mk(); T("S with again", () => S_with(c), c); }
{ const h = new F_with(); h.k = mk(); T("F with again", () => h.run(), h.k); }
{ G = mk(); T("G with again", () => G_with(), G); }
{ const c: number[] = mk(); T("L with again", () => c.with(0, 9), c); }

// The universal dispatcher: an array-like without the method throws, one that
// borrows or inherits the builtin runs it.
interface H { list: number[] }
function D(label: string, f: () => any): void {
  let r: string;
  try {
    r = String(f());
  } catch (e: any) {
    r = "THROW " + (e && e.name);
  }
  console.log(label + " -> " + r);
}
for (const m of ["push", "pop", "shift", "unshift", "splice", "sort", "reverse", "concat"]) {
  const o: any = { length: 1, 0: 5 };
  D("dyn none " + m, () => o[m](1));
  D("dyn none " + m + " after", () => st(o));
  const b: any = { length: 1, 0: 5 };
  b[m] = (Array.prototype as any)[m];
  D("dyn lent " + m, () => typeof b[m](1));
  const p: any = Object.create(Array.prototype);
  D("dyn inherits " + m, () => typeof p[m](1));
}
{ const h: H = { list: { length: 0 } as any }; D("holder push", () => h.list.push(7)); D("holder length", () => h.list.length); }
{ const h: H = { list: { length: 0 } as any }; D("holder pop", () => h.list.pop()); }
{ const h: H = { list: { length: 0 } as any }; D("holder shift", () => h.list.shift()); }
{ const h: H = { list: { length: 0 } as any }; D("holder unshift", () => h.list.unshift(1)); }
{ const h: H = { list: { length: 0 } as any }; D("holder sort", () => h.list.sort()); }
{ const h: H = { list: [1, 2] }; D("holder real push", () => h.list.push(7)); D("holder real pop", () => h.list.pop()); D("holder real", () => h.list.join()); }
{ const o: any = { length: 0 }; D("any push", () => o.push(7)); D("any pop", () => o.pop()); D("any length", () => o.length); }
{ const o: any = { length: 0 }; D("borrowed call push", () => Array.prototype.push.call(o, 7) + " " + o[0] + " " + o.length); D("borrowed call pop", () => Array.prototype.pop.call(o)); }
class Stack { items: number[] = []; push(x: number): string { this.items.push(x); return "user"; } }
{ const s: any = new Stack(); D("class push", () => s.push(1)); D("class pop", () => s.pop()); }

// Controls: plain Arrays through the same sites stay right (sums of the
// results over many calls).
function drain(a: number[]): number { let s = 0; while (a.length > 0) s += a.pop()!; return s; }
function fillTo(n: number): number[] { const a: number[] = []; for (let i = 0; i < n; i++) a.push(i); return a; }
class Pool { free: number[] = []; take(): number { const v = this.free.pop(); return v === undefined ? -1 : v; } give(v: number): number { return this.free.push(v); } }
{
  let s = 0;
  for (let r = 0; r < 50; r++) s += drain(fillTo(40));
  console.log("control drain " + s);
  const p = new Pool();
  let t = 0;
  for (let i = 0; i < 300; i++) t += p.give(i);
  for (let i = 0; i < 310; i++) t += p.take();
  console.log("control pool " + t);
  G = fillTo(30);
  let u = 0;
  for (let i = 0; i < 20; i++) { u += G_pop(); u += G_indexOf(); u += G_includes() ? 1 : 0; u += G_slice().length; u += G_unshift(); }
  console.log("control module " + u + " " + G.length);
  const e: number[] = [];
  console.log("control empty " + String(e.pop()) + " " + String(P_pop(e)) + " " + String(e.shift()) + " " + e.length);
  const strs: string[] = ["b", "a"];
  console.log("control strings " + String(strs.pop()) + " " + strs.join() + " " + strs.indexOf("b") + " " + strs.includes("a"));
  console.log("control calls " + calls + " sink " + sink);
}
