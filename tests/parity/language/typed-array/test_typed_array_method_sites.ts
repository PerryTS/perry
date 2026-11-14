// A method call on a receiver ANNOTATED a typed array runs what the VALUE holds: the built-in for a
// typed array the realm bore, the object's or class's own method for an ordinary object, a class instance
// or a subclass instance behind the annotation, and a TypeError for null / undefined. Rows: kind x method
// x receiver (real, fake, cls, sub, psub, null, undef) x site (P parameter, S parameter in statement
// position, F field, G module binding, L local). `c=` is the number of user methods the site ran.
let sink = 0; let calls = 0;
function st(a: any): string { if (a === null || a === undefined) return String(a); let s = ArrayBuffer.isView(a) ? "T[" : Array.isArray(a) ? "A[" : "O["; const n = a.length; for (let i = 0; i < n; i++) s += a[i] + ","; return s + "]"; }
function show(r: any): string { if (r !== null && typeof r === "object") { if (typeof r.next === "function") { let s = "I["; for (const v of r) s += v + ";"; return s + "]"; } return st(r); } return typeof r + ":" + String(r); }
function T(label: string, f: () => any, a: any): void { const c0 = calls; let r: string; try { r = show(f()); } catch (e: any) { r = "THROW " + (e && e.name); } console.log(label + " -> " + r + " | " + st(a) + " c=" + (calls - c0)); }
function mkF64(): Float64Array { const a = new Float64Array(3); a[0] = 3; a[1] = 1; a[2] = 2; return a; }
class SubF64 extends Float64Array {
  at(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  fill(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  indexOf(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  includes(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  slice(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  join(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  reverse(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  map(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  filter(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  forEach(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  some(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  find(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  reduce(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  sort(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  keys(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  values(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  with(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  toSorted(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  set(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  subarray(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
}
class PlainF64 extends Float64Array { tag(): string { return "plain-sub"; } }
class KF64 { length = 0;
  at(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  fill(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  indexOf(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  includes(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  slice(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  join(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  reverse(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  map(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  filter(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  forEach(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  some(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  find(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  reduce(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  sort(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  keys(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  values(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  with(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  toSorted(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  set(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  subarray(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
}
function subF64(): Float64Array { const s: any = new SubF64(3); s[0] = 3; s[1] = 1; s[2] = 2; return s; }
function psubF64(): Float64Array { const s: any = new PlainF64(3); s[0] = 3; s[1] = 1; s[2] = 2; return s; }
function fakeF64(): Float64Array { const o: any = { length: 0 };
  o.at = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.fill = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.indexOf = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.includes = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.slice = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.join = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.reverse = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.map = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.filter = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.forEach = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.some = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.find = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.reduce = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.sort = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.keys = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.values = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.with = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.toSorted = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.set = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.subarray = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  return o as Float64Array; }
function clsF64(): Float64Array { const o: any = new KF64(); return o as Float64Array; }
function nullF64(): Float64Array { const o: any = JSON.parse("null"); return o as Float64Array; }
function undefF64(): Float64Array { const o: any = JSON.parse("[]")[0]; return o as Float64Array; }
function PF64_at(a: Float64Array): any { return a.at(0); }
function SF64_at(a: Float64Array): void { a.at(0); }
class FF64_at { k: Float64Array = mkF64(); run(): any { return this.k.at(0); } }
function PF64_fill(a: Float64Array): any { return a.fill(5); }
function SF64_fill(a: Float64Array): void { a.fill(5); }
class FF64_fill { k: Float64Array = mkF64(); run(): any { return this.k.fill(5); } }
function PF64_indexOf(a: Float64Array): any { return a.indexOf(1); }
function SF64_indexOf(a: Float64Array): void { a.indexOf(1); }
class FF64_indexOf { k: Float64Array = mkF64(); run(): any { return this.k.indexOf(1); } }
function PF64_includes(a: Float64Array): any { return a.includes(1); }
function SF64_includes(a: Float64Array): void { a.includes(1); }
class FF64_includes { k: Float64Array = mkF64(); run(): any { return this.k.includes(1); } }
function PF64_slice(a: Float64Array): any { return a.slice(0, 2); }
function SF64_slice(a: Float64Array): void { a.slice(0, 2); }
class FF64_slice { k: Float64Array = mkF64(); run(): any { return this.k.slice(0, 2); } }
function PF64_join(a: Float64Array): any { return a.join("-"); }
function SF64_join(a: Float64Array): void { a.join("-"); }
class FF64_join { k: Float64Array = mkF64(); run(): any { return this.k.join("-"); } }
function PF64_reverse(a: Float64Array): any { return a.reverse(); }
function SF64_reverse(a: Float64Array): void { a.reverse(); }
class FF64_reverse { k: Float64Array = mkF64(); run(): any { return this.k.reverse(); } }
function PF64_map(a: Float64Array): any { return a.map((x: any) => x); }
function SF64_map(a: Float64Array): void { a.map((x: any) => x); }
class FF64_map { k: Float64Array = mkF64(); run(): any { return this.k.map((x: any) => x); } }
function PF64_filter(a: Float64Array): any { return a.filter((x: any) => x > 1); }
function SF64_filter(a: Float64Array): void { a.filter((x: any) => x > 1); }
class FF64_filter { k: Float64Array = mkF64(); run(): any { return this.k.filter((x: any) => x > 1); } }
function PF64_forEach(a: Float64Array): any { return a.forEach((x: any) => { sink += Number(x); }); }
function SF64_forEach(a: Float64Array): void { a.forEach((x: any) => { sink += Number(x); }); }
class FF64_forEach { k: Float64Array = mkF64(); run(): any { return this.k.forEach((x: any) => { sink += Number(x); }); } }
function PF64_some(a: Float64Array): any { return a.some((x: any) => x > 1); }
function SF64_some(a: Float64Array): void { a.some((x: any) => x > 1); }
class FF64_some { k: Float64Array = mkF64(); run(): any { return this.k.some((x: any) => x > 1); } }
function PF64_find(a: Float64Array): any { return a.find((x: any) => x > 1); }
function SF64_find(a: Float64Array): void { a.find((x: any) => x > 1); }
class FF64_find { k: Float64Array = mkF64(); run(): any { return this.k.find((x: any) => x > 1); } }
function PF64_reduce(a: Float64Array): any { return a.reduce((p: any, x: any) => p + x, 0); }
function SF64_reduce(a: Float64Array): void { a.reduce((p: any, x: any) => p + x, 0); }
class FF64_reduce { k: Float64Array = mkF64(); run(): any { return this.k.reduce((p: any, x: any) => p + x, 0); } }
function PF64_sort(a: Float64Array): any { return a.sort(); }
function SF64_sort(a: Float64Array): void { a.sort(); }
class FF64_sort { k: Float64Array = mkF64(); run(): any { return this.k.sort(); } }
function PF64_keys(a: Float64Array): any { return a.keys(); }
function SF64_keys(a: Float64Array): void { a.keys(); }
class FF64_keys { k: Float64Array = mkF64(); run(): any { return this.k.keys(); } }
function PF64_values(a: Float64Array): any { return a.values(); }
function SF64_values(a: Float64Array): void { a.values(); }
class FF64_values { k: Float64Array = mkF64(); run(): any { return this.k.values(); } }
function PF64_with(a: Float64Array): any { return a.with(0, 9); }
function SF64_with(a: Float64Array): void { a.with(0, 9); }
class FF64_with { k: Float64Array = mkF64(); run(): any { return this.k.with(0, 9); } }
function PF64_toSorted(a: Float64Array): any { return a.toSorted(); }
function SF64_toSorted(a: Float64Array): void { a.toSorted(); }
class FF64_toSorted { k: Float64Array = mkF64(); run(): any { return this.k.toSorted(); } }
function PF64_set(a: Float64Array): any { return a.set([1]); }
function SF64_set(a: Float64Array): void { a.set([1]); }
class FF64_set { k: Float64Array = mkF64(); run(): any { return this.k.set([1]); } }
function PF64_subarray(a: Float64Array): any { return a.subarray(1); }
function SF64_subarray(a: Float64Array): void { a.subarray(1); }
class FF64_subarray { k: Float64Array = mkF64(); run(): any { return this.k.subarray(1); } }
let GF64: Float64Array = mkF64();
function GF64_at(): any { return GF64.at(0); }
function GF64_fill(): any { return GF64.fill(5); }
function GF64_indexOf(): any { return GF64.indexOf(1); }
function GF64_includes(): any { return GF64.includes(1); }
function GF64_slice(): any { return GF64.slice(0, 2); }
function GF64_join(): any { return GF64.join("-"); }
function GF64_reverse(): any { return GF64.reverse(); }
function GF64_map(): any { return GF64.map((x: any) => x); }
function GF64_filter(): any { return GF64.filter((x: any) => x > 1); }
function GF64_forEach(): any { return GF64.forEach((x: any) => { sink += Number(x); }); }
function GF64_some(): any { return GF64.some((x: any) => x > 1); }
function GF64_find(): any { return GF64.find((x: any) => x > 1); }
function GF64_reduce(): any { return GF64.reduce((p: any, x: any) => p + x, 0); }
function GF64_sort(): any { return GF64.sort(); }
function GF64_keys(): any { return GF64.keys(); }
function GF64_values(): any { return GF64.values(); }
function GF64_with(): any { return GF64.with(0, 9); }
function GF64_toSorted(): any { return GF64.toSorted(); }
function GF64_set(): any { return GF64.set([1]); }
function GF64_subarray(): any { return GF64.subarray(1); }
{ const c: Float64Array = mkF64(); T("F64 P at real", () => PF64_at(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S at real", () => { SF64_at(c); return 0; }, c); }
{ const h = new FF64_at(); const c: Float64Array = mkF64(); h.k = c; T("F64 F at real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G at real", () => GF64_at(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L at real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P fill real", () => PF64_fill(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S fill real", () => { SF64_fill(c); return 0; }, c); }
{ const h = new FF64_fill(); const c: Float64Array = mkF64(); h.k = c; T("F64 F fill real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G fill real", () => GF64_fill(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L fill real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P indexOf real", () => PF64_indexOf(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S indexOf real", () => { SF64_indexOf(c); return 0; }, c); }
{ const h = new FF64_indexOf(); const c: Float64Array = mkF64(); h.k = c; T("F64 F indexOf real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G indexOf real", () => GF64_indexOf(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L indexOf real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P includes real", () => PF64_includes(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S includes real", () => { SF64_includes(c); return 0; }, c); }
{ const h = new FF64_includes(); const c: Float64Array = mkF64(); h.k = c; T("F64 F includes real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G includes real", () => GF64_includes(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L includes real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P slice real", () => PF64_slice(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S slice real", () => { SF64_slice(c); return 0; }, c); }
{ const h = new FF64_slice(); const c: Float64Array = mkF64(); h.k = c; T("F64 F slice real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G slice real", () => GF64_slice(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L slice real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P join real", () => PF64_join(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S join real", () => { SF64_join(c); return 0; }, c); }
{ const h = new FF64_join(); const c: Float64Array = mkF64(); h.k = c; T("F64 F join real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G join real", () => GF64_join(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L join real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P reverse real", () => PF64_reverse(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S reverse real", () => { SF64_reverse(c); return 0; }, c); }
{ const h = new FF64_reverse(); const c: Float64Array = mkF64(); h.k = c; T("F64 F reverse real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G reverse real", () => GF64_reverse(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reverse real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P map real", () => PF64_map(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S map real", () => { SF64_map(c); return 0; }, c); }
{ const h = new FF64_map(); const c: Float64Array = mkF64(); h.k = c; T("F64 F map real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G map real", () => GF64_map(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L map real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P filter real", () => PF64_filter(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S filter real", () => { SF64_filter(c); return 0; }, c); }
{ const h = new FF64_filter(); const c: Float64Array = mkF64(); h.k = c; T("F64 F filter real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G filter real", () => GF64_filter(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L filter real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P forEach real", () => PF64_forEach(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S forEach real", () => { SF64_forEach(c); return 0; }, c); }
{ const h = new FF64_forEach(); const c: Float64Array = mkF64(); h.k = c; T("F64 F forEach real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G forEach real", () => GF64_forEach(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L forEach real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P some real", () => PF64_some(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S some real", () => { SF64_some(c); return 0; }, c); }
{ const h = new FF64_some(); const c: Float64Array = mkF64(); h.k = c; T("F64 F some real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G some real", () => GF64_some(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L some real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P find real", () => PF64_find(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S find real", () => { SF64_find(c); return 0; }, c); }
{ const h = new FF64_find(); const c: Float64Array = mkF64(); h.k = c; T("F64 F find real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G find real", () => GF64_find(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L find real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P reduce real", () => PF64_reduce(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S reduce real", () => { SF64_reduce(c); return 0; }, c); }
{ const h = new FF64_reduce(); const c: Float64Array = mkF64(); h.k = c; T("F64 F reduce real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G reduce real", () => GF64_reduce(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reduce real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P sort real", () => PF64_sort(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S sort real", () => { SF64_sort(c); return 0; }, c); }
{ const h = new FF64_sort(); const c: Float64Array = mkF64(); h.k = c; T("F64 F sort real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G sort real", () => GF64_sort(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L sort real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P keys real", () => PF64_keys(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S keys real", () => { SF64_keys(c); return 0; }, c); }
{ const h = new FF64_keys(); const c: Float64Array = mkF64(); h.k = c; T("F64 F keys real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G keys real", () => GF64_keys(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L keys real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P values real", () => PF64_values(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S values real", () => { SF64_values(c); return 0; }, c); }
{ const h = new FF64_values(); const c: Float64Array = mkF64(); h.k = c; T("F64 F values real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G values real", () => GF64_values(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L values real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P with real", () => PF64_with(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S with real", () => { SF64_with(c); return 0; }, c); }
{ const h = new FF64_with(); const c: Float64Array = mkF64(); h.k = c; T("F64 F with real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G with real", () => GF64_with(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L with real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P toSorted real", () => PF64_toSorted(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S toSorted real", () => { SF64_toSorted(c); return 0; }, c); }
{ const h = new FF64_toSorted(); const c: Float64Array = mkF64(); h.k = c; T("F64 F toSorted real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G toSorted real", () => GF64_toSorted(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L toSorted real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P set real", () => PF64_set(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S set real", () => { SF64_set(c); return 0; }, c); }
{ const h = new FF64_set(); const c: Float64Array = mkF64(); h.k = c; T("F64 F set real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G set real", () => GF64_set(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L set real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = mkF64(); T("F64 P subarray real", () => PF64_subarray(c), c); }
{ const c: Float64Array = mkF64(); T("F64 S subarray real", () => { SF64_subarray(c); return 0; }, c); }
{ const h = new FF64_subarray(); const c: Float64Array = mkF64(); h.k = c; T("F64 F subarray real", () => h.run(), h.k); }
{ GF64 = mkF64(); T("F64 G subarray real", () => GF64_subarray(), GF64); }
{ const c: Float64Array = mkF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L subarray real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P at fake", () => PF64_at(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S at fake", () => { SF64_at(c); return 0; }, c); }
{ const h = new FF64_at(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F at fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G at fake", () => GF64_at(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L at fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P fill fake", () => PF64_fill(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S fill fake", () => { SF64_fill(c); return 0; }, c); }
{ const h = new FF64_fill(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F fill fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G fill fake", () => GF64_fill(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L fill fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P indexOf fake", () => PF64_indexOf(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S indexOf fake", () => { SF64_indexOf(c); return 0; }, c); }
{ const h = new FF64_indexOf(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F indexOf fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G indexOf fake", () => GF64_indexOf(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L indexOf fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P includes fake", () => PF64_includes(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S includes fake", () => { SF64_includes(c); return 0; }, c); }
{ const h = new FF64_includes(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F includes fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G includes fake", () => GF64_includes(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L includes fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P slice fake", () => PF64_slice(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S slice fake", () => { SF64_slice(c); return 0; }, c); }
{ const h = new FF64_slice(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F slice fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G slice fake", () => GF64_slice(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L slice fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P join fake", () => PF64_join(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S join fake", () => { SF64_join(c); return 0; }, c); }
{ const h = new FF64_join(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F join fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G join fake", () => GF64_join(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L join fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P reverse fake", () => PF64_reverse(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S reverse fake", () => { SF64_reverse(c); return 0; }, c); }
{ const h = new FF64_reverse(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F reverse fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G reverse fake", () => GF64_reverse(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reverse fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P map fake", () => PF64_map(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S map fake", () => { SF64_map(c); return 0; }, c); }
{ const h = new FF64_map(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F map fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G map fake", () => GF64_map(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L map fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P filter fake", () => PF64_filter(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S filter fake", () => { SF64_filter(c); return 0; }, c); }
{ const h = new FF64_filter(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F filter fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G filter fake", () => GF64_filter(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L filter fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P forEach fake", () => PF64_forEach(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S forEach fake", () => { SF64_forEach(c); return 0; }, c); }
{ const h = new FF64_forEach(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F forEach fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G forEach fake", () => GF64_forEach(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L forEach fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P some fake", () => PF64_some(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S some fake", () => { SF64_some(c); return 0; }, c); }
{ const h = new FF64_some(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F some fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G some fake", () => GF64_some(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L some fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P find fake", () => PF64_find(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S find fake", () => { SF64_find(c); return 0; }, c); }
{ const h = new FF64_find(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F find fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G find fake", () => GF64_find(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L find fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P reduce fake", () => PF64_reduce(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S reduce fake", () => { SF64_reduce(c); return 0; }, c); }
{ const h = new FF64_reduce(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F reduce fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G reduce fake", () => GF64_reduce(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reduce fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P sort fake", () => PF64_sort(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S sort fake", () => { SF64_sort(c); return 0; }, c); }
{ const h = new FF64_sort(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F sort fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G sort fake", () => GF64_sort(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L sort fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P keys fake", () => PF64_keys(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S keys fake", () => { SF64_keys(c); return 0; }, c); }
{ const h = new FF64_keys(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F keys fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G keys fake", () => GF64_keys(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L keys fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P values fake", () => PF64_values(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S values fake", () => { SF64_values(c); return 0; }, c); }
{ const h = new FF64_values(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F values fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G values fake", () => GF64_values(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L values fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P with fake", () => PF64_with(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S with fake", () => { SF64_with(c); return 0; }, c); }
{ const h = new FF64_with(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F with fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G with fake", () => GF64_with(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L with fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P toSorted fake", () => PF64_toSorted(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S toSorted fake", () => { SF64_toSorted(c); return 0; }, c); }
{ const h = new FF64_toSorted(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F toSorted fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G toSorted fake", () => GF64_toSorted(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L toSorted fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P set fake", () => PF64_set(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S set fake", () => { SF64_set(c); return 0; }, c); }
{ const h = new FF64_set(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F set fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G set fake", () => GF64_set(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L set fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = fakeF64(); T("F64 P subarray fake", () => PF64_subarray(c), c); }
{ const c: Float64Array = fakeF64(); T("F64 S subarray fake", () => { SF64_subarray(c); return 0; }, c); }
{ const h = new FF64_subarray(); const c: Float64Array = fakeF64(); h.k = c; T("F64 F subarray fake", () => h.run(), h.k); }
{ GF64 = fakeF64(); T("F64 G subarray fake", () => GF64_subarray(), GF64); }
{ const c: Float64Array = fakeF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L subarray fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P at cls", () => PF64_at(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S at cls", () => { SF64_at(c); return 0; }, c); }
{ const h = new FF64_at(); const c: Float64Array = clsF64(); h.k = c; T("F64 F at cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G at cls", () => GF64_at(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L at cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P fill cls", () => PF64_fill(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S fill cls", () => { SF64_fill(c); return 0; }, c); }
{ const h = new FF64_fill(); const c: Float64Array = clsF64(); h.k = c; T("F64 F fill cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G fill cls", () => GF64_fill(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L fill cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P indexOf cls", () => PF64_indexOf(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S indexOf cls", () => { SF64_indexOf(c); return 0; }, c); }
{ const h = new FF64_indexOf(); const c: Float64Array = clsF64(); h.k = c; T("F64 F indexOf cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G indexOf cls", () => GF64_indexOf(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L indexOf cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P includes cls", () => PF64_includes(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S includes cls", () => { SF64_includes(c); return 0; }, c); }
{ const h = new FF64_includes(); const c: Float64Array = clsF64(); h.k = c; T("F64 F includes cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G includes cls", () => GF64_includes(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L includes cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P slice cls", () => PF64_slice(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S slice cls", () => { SF64_slice(c); return 0; }, c); }
{ const h = new FF64_slice(); const c: Float64Array = clsF64(); h.k = c; T("F64 F slice cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G slice cls", () => GF64_slice(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L slice cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P join cls", () => PF64_join(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S join cls", () => { SF64_join(c); return 0; }, c); }
{ const h = new FF64_join(); const c: Float64Array = clsF64(); h.k = c; T("F64 F join cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G join cls", () => GF64_join(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L join cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P reverse cls", () => PF64_reverse(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S reverse cls", () => { SF64_reverse(c); return 0; }, c); }
{ const h = new FF64_reverse(); const c: Float64Array = clsF64(); h.k = c; T("F64 F reverse cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G reverse cls", () => GF64_reverse(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reverse cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P map cls", () => PF64_map(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S map cls", () => { SF64_map(c); return 0; }, c); }
{ const h = new FF64_map(); const c: Float64Array = clsF64(); h.k = c; T("F64 F map cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G map cls", () => GF64_map(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L map cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P filter cls", () => PF64_filter(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S filter cls", () => { SF64_filter(c); return 0; }, c); }
{ const h = new FF64_filter(); const c: Float64Array = clsF64(); h.k = c; T("F64 F filter cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G filter cls", () => GF64_filter(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L filter cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P forEach cls", () => PF64_forEach(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S forEach cls", () => { SF64_forEach(c); return 0; }, c); }
{ const h = new FF64_forEach(); const c: Float64Array = clsF64(); h.k = c; T("F64 F forEach cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G forEach cls", () => GF64_forEach(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L forEach cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P some cls", () => PF64_some(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S some cls", () => { SF64_some(c); return 0; }, c); }
{ const h = new FF64_some(); const c: Float64Array = clsF64(); h.k = c; T("F64 F some cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G some cls", () => GF64_some(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L some cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P find cls", () => PF64_find(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S find cls", () => { SF64_find(c); return 0; }, c); }
{ const h = new FF64_find(); const c: Float64Array = clsF64(); h.k = c; T("F64 F find cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G find cls", () => GF64_find(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L find cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P reduce cls", () => PF64_reduce(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S reduce cls", () => { SF64_reduce(c); return 0; }, c); }
{ const h = new FF64_reduce(); const c: Float64Array = clsF64(); h.k = c; T("F64 F reduce cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G reduce cls", () => GF64_reduce(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reduce cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P sort cls", () => PF64_sort(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S sort cls", () => { SF64_sort(c); return 0; }, c); }
{ const h = new FF64_sort(); const c: Float64Array = clsF64(); h.k = c; T("F64 F sort cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G sort cls", () => GF64_sort(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L sort cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P keys cls", () => PF64_keys(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S keys cls", () => { SF64_keys(c); return 0; }, c); }
{ const h = new FF64_keys(); const c: Float64Array = clsF64(); h.k = c; T("F64 F keys cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G keys cls", () => GF64_keys(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L keys cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P values cls", () => PF64_values(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S values cls", () => { SF64_values(c); return 0; }, c); }
{ const h = new FF64_values(); const c: Float64Array = clsF64(); h.k = c; T("F64 F values cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G values cls", () => GF64_values(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L values cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P with cls", () => PF64_with(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S with cls", () => { SF64_with(c); return 0; }, c); }
{ const h = new FF64_with(); const c: Float64Array = clsF64(); h.k = c; T("F64 F with cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G with cls", () => GF64_with(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L with cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P toSorted cls", () => PF64_toSorted(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S toSorted cls", () => { SF64_toSorted(c); return 0; }, c); }
{ const h = new FF64_toSorted(); const c: Float64Array = clsF64(); h.k = c; T("F64 F toSorted cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G toSorted cls", () => GF64_toSorted(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L toSorted cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P set cls", () => PF64_set(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S set cls", () => { SF64_set(c); return 0; }, c); }
{ const h = new FF64_set(); const c: Float64Array = clsF64(); h.k = c; T("F64 F set cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G set cls", () => GF64_set(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L set cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = clsF64(); T("F64 P subarray cls", () => PF64_subarray(c), c); }
{ const c: Float64Array = clsF64(); T("F64 S subarray cls", () => { SF64_subarray(c); return 0; }, c); }
{ const h = new FF64_subarray(); const c: Float64Array = clsF64(); h.k = c; T("F64 F subarray cls", () => h.run(), h.k); }
{ GF64 = clsF64(); T("F64 G subarray cls", () => GF64_subarray(), GF64); }
{ const c: Float64Array = clsF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L subarray cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P at sub", () => PF64_at(c), c); }
{ const c: Float64Array = subF64(); T("F64 S at sub", () => { SF64_at(c); return 0; }, c); }
{ const h = new FF64_at(); const c: Float64Array = subF64(); h.k = c; T("F64 F at sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L at sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P fill sub", () => PF64_fill(c), c); }
{ const c: Float64Array = subF64(); T("F64 S fill sub", () => { SF64_fill(c); return 0; }, c); }
{ const h = new FF64_fill(); const c: Float64Array = subF64(); h.k = c; T("F64 F fill sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L fill sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P indexOf sub", () => PF64_indexOf(c), c); }
{ const c: Float64Array = subF64(); T("F64 S indexOf sub", () => { SF64_indexOf(c); return 0; }, c); }
{ const h = new FF64_indexOf(); const c: Float64Array = subF64(); h.k = c; T("F64 F indexOf sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L indexOf sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P includes sub", () => PF64_includes(c), c); }
{ const c: Float64Array = subF64(); T("F64 S includes sub", () => { SF64_includes(c); return 0; }, c); }
{ const h = new FF64_includes(); const c: Float64Array = subF64(); h.k = c; T("F64 F includes sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L includes sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P slice sub", () => PF64_slice(c), c); }
{ const c: Float64Array = subF64(); T("F64 S slice sub", () => { SF64_slice(c); return 0; }, c); }
{ const h = new FF64_slice(); const c: Float64Array = subF64(); h.k = c; T("F64 F slice sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L slice sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P join sub", () => PF64_join(c), c); }
{ const c: Float64Array = subF64(); T("F64 S join sub", () => { SF64_join(c); return 0; }, c); }
{ const h = new FF64_join(); const c: Float64Array = subF64(); h.k = c; T("F64 F join sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L join sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P reverse sub", () => PF64_reverse(c), c); }
{ const c: Float64Array = subF64(); T("F64 S reverse sub", () => { SF64_reverse(c); return 0; }, c); }
{ const h = new FF64_reverse(); const c: Float64Array = subF64(); h.k = c; T("F64 F reverse sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reverse sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P map sub", () => PF64_map(c), c); }
{ const c: Float64Array = subF64(); T("F64 S map sub", () => { SF64_map(c); return 0; }, c); }
{ const h = new FF64_map(); const c: Float64Array = subF64(); h.k = c; T("F64 F map sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L map sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P filter sub", () => PF64_filter(c), c); }
{ const c: Float64Array = subF64(); T("F64 S filter sub", () => { SF64_filter(c); return 0; }, c); }
{ const h = new FF64_filter(); const c: Float64Array = subF64(); h.k = c; T("F64 F filter sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L filter sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P forEach sub", () => PF64_forEach(c), c); }
{ const c: Float64Array = subF64(); T("F64 S forEach sub", () => { SF64_forEach(c); return 0; }, c); }
{ const h = new FF64_forEach(); const c: Float64Array = subF64(); h.k = c; T("F64 F forEach sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L forEach sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P some sub", () => PF64_some(c), c); }
{ const c: Float64Array = subF64(); T("F64 S some sub", () => { SF64_some(c); return 0; }, c); }
{ const h = new FF64_some(); const c: Float64Array = subF64(); h.k = c; T("F64 F some sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L some sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P find sub", () => PF64_find(c), c); }
{ const c: Float64Array = subF64(); T("F64 S find sub", () => { SF64_find(c); return 0; }, c); }
{ const h = new FF64_find(); const c: Float64Array = subF64(); h.k = c; T("F64 F find sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L find sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P reduce sub", () => PF64_reduce(c), c); }
{ const c: Float64Array = subF64(); T("F64 S reduce sub", () => { SF64_reduce(c); return 0; }, c); }
{ const h = new FF64_reduce(); const c: Float64Array = subF64(); h.k = c; T("F64 F reduce sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reduce sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P sort sub", () => PF64_sort(c), c); }
{ const c: Float64Array = subF64(); T("F64 S sort sub", () => { SF64_sort(c); return 0; }, c); }
{ const h = new FF64_sort(); const c: Float64Array = subF64(); h.k = c; T("F64 F sort sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L sort sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P keys sub", () => PF64_keys(c), c); }
{ const c: Float64Array = subF64(); T("F64 S keys sub", () => { SF64_keys(c); return 0; }, c); }
{ const h = new FF64_keys(); const c: Float64Array = subF64(); h.k = c; T("F64 F keys sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L keys sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P values sub", () => PF64_values(c), c); }
{ const c: Float64Array = subF64(); T("F64 S values sub", () => { SF64_values(c); return 0; }, c); }
{ const h = new FF64_values(); const c: Float64Array = subF64(); h.k = c; T("F64 F values sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L values sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P with sub", () => PF64_with(c), c); }
{ const c: Float64Array = subF64(); T("F64 S with sub", () => { SF64_with(c); return 0; }, c); }
{ const h = new FF64_with(); const c: Float64Array = subF64(); h.k = c; T("F64 F with sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L with sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P toSorted sub", () => PF64_toSorted(c), c); }
{ const c: Float64Array = subF64(); T("F64 S toSorted sub", () => { SF64_toSorted(c); return 0; }, c); }
{ const h = new FF64_toSorted(); const c: Float64Array = subF64(); h.k = c; T("F64 F toSorted sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L toSorted sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P set sub", () => PF64_set(c), c); }
{ const c: Float64Array = subF64(); T("F64 S set sub", () => { SF64_set(c); return 0; }, c); }
{ const h = new FF64_set(); const c: Float64Array = subF64(); h.k = c; T("F64 F set sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L set sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = subF64(); T("F64 P subarray sub", () => PF64_subarray(c), c); }
{ const c: Float64Array = subF64(); T("F64 S subarray sub", () => { SF64_subarray(c); return 0; }, c); }
{ const h = new FF64_subarray(); const c: Float64Array = subF64(); h.k = c; T("F64 F subarray sub", () => h.run(), h.k); }
{ const c: Float64Array = subF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L subarray sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P at psub", () => PF64_at(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S at psub", () => { SF64_at(c); return 0; }, c); }
{ const h = new FF64_at(); const c: Float64Array = psubF64(); h.k = c; T("F64 F at psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G at psub", () => GF64_at(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L at psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P fill psub", () => PF64_fill(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S fill psub", () => { SF64_fill(c); return 0; }, c); }
{ const h = new FF64_fill(); const c: Float64Array = psubF64(); h.k = c; T("F64 F fill psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G fill psub", () => GF64_fill(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L fill psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P indexOf psub", () => PF64_indexOf(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S indexOf psub", () => { SF64_indexOf(c); return 0; }, c); }
{ const h = new FF64_indexOf(); const c: Float64Array = psubF64(); h.k = c; T("F64 F indexOf psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G indexOf psub", () => GF64_indexOf(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L indexOf psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P includes psub", () => PF64_includes(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S includes psub", () => { SF64_includes(c); return 0; }, c); }
{ const h = new FF64_includes(); const c: Float64Array = psubF64(); h.k = c; T("F64 F includes psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G includes psub", () => GF64_includes(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L includes psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P slice psub", () => PF64_slice(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S slice psub", () => { SF64_slice(c); return 0; }, c); }
{ const h = new FF64_slice(); const c: Float64Array = psubF64(); h.k = c; T("F64 F slice psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G slice psub", () => GF64_slice(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L slice psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P join psub", () => PF64_join(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S join psub", () => { SF64_join(c); return 0; }, c); }
{ const h = new FF64_join(); const c: Float64Array = psubF64(); h.k = c; T("F64 F join psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G join psub", () => GF64_join(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L join psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P reverse psub", () => PF64_reverse(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S reverse psub", () => { SF64_reverse(c); return 0; }, c); }
{ const h = new FF64_reverse(); const c: Float64Array = psubF64(); h.k = c; T("F64 F reverse psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G reverse psub", () => GF64_reverse(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reverse psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P map psub", () => PF64_map(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S map psub", () => { SF64_map(c); return 0; }, c); }
{ const h = new FF64_map(); const c: Float64Array = psubF64(); h.k = c; T("F64 F map psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G map psub", () => GF64_map(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L map psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P filter psub", () => PF64_filter(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S filter psub", () => { SF64_filter(c); return 0; }, c); }
{ const h = new FF64_filter(); const c: Float64Array = psubF64(); h.k = c; T("F64 F filter psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G filter psub", () => GF64_filter(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L filter psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P forEach psub", () => PF64_forEach(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S forEach psub", () => { SF64_forEach(c); return 0; }, c); }
{ const h = new FF64_forEach(); const c: Float64Array = psubF64(); h.k = c; T("F64 F forEach psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G forEach psub", () => GF64_forEach(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L forEach psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P some psub", () => PF64_some(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S some psub", () => { SF64_some(c); return 0; }, c); }
{ const h = new FF64_some(); const c: Float64Array = psubF64(); h.k = c; T("F64 F some psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G some psub", () => GF64_some(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L some psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P find psub", () => PF64_find(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S find psub", () => { SF64_find(c); return 0; }, c); }
{ const h = new FF64_find(); const c: Float64Array = psubF64(); h.k = c; T("F64 F find psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G find psub", () => GF64_find(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L find psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P reduce psub", () => PF64_reduce(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S reduce psub", () => { SF64_reduce(c); return 0; }, c); }
{ const h = new FF64_reduce(); const c: Float64Array = psubF64(); h.k = c; T("F64 F reduce psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G reduce psub", () => GF64_reduce(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reduce psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P sort psub", () => PF64_sort(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S sort psub", () => { SF64_sort(c); return 0; }, c); }
{ const h = new FF64_sort(); const c: Float64Array = psubF64(); h.k = c; T("F64 F sort psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G sort psub", () => GF64_sort(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L sort psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P keys psub", () => PF64_keys(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S keys psub", () => { SF64_keys(c); return 0; }, c); }
{ const h = new FF64_keys(); const c: Float64Array = psubF64(); h.k = c; T("F64 F keys psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G keys psub", () => GF64_keys(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L keys psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P values psub", () => PF64_values(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S values psub", () => { SF64_values(c); return 0; }, c); }
{ const h = new FF64_values(); const c: Float64Array = psubF64(); h.k = c; T("F64 F values psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G values psub", () => GF64_values(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L values psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P with psub", () => PF64_with(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S with psub", () => { SF64_with(c); return 0; }, c); }
{ const h = new FF64_with(); const c: Float64Array = psubF64(); h.k = c; T("F64 F with psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G with psub", () => GF64_with(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L with psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P toSorted psub", () => PF64_toSorted(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S toSorted psub", () => { SF64_toSorted(c); return 0; }, c); }
{ const h = new FF64_toSorted(); const c: Float64Array = psubF64(); h.k = c; T("F64 F toSorted psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G toSorted psub", () => GF64_toSorted(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L toSorted psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P set psub", () => PF64_set(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S set psub", () => { SF64_set(c); return 0; }, c); }
{ const h = new FF64_set(); const c: Float64Array = psubF64(); h.k = c; T("F64 F set psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G set psub", () => GF64_set(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L set psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = psubF64(); T("F64 P subarray psub", () => PF64_subarray(c), c); }
{ const c: Float64Array = psubF64(); T("F64 S subarray psub", () => { SF64_subarray(c); return 0; }, c); }
{ const h = new FF64_subarray(); const c: Float64Array = psubF64(); h.k = c; T("F64 F subarray psub", () => h.run(), h.k); }
{ GF64 = psubF64(); T("F64 G subarray psub", () => GF64_subarray(), GF64); }
{ const c: Float64Array = psubF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L subarray psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P at null", () => PF64_at(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S at null", () => { SF64_at(c); return 0; }, c); }
{ const h = new FF64_at(); const c: Float64Array = nullF64(); h.k = c; T("F64 F at null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L at null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P fill null", () => PF64_fill(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S fill null", () => { SF64_fill(c); return 0; }, c); }
{ const h = new FF64_fill(); const c: Float64Array = nullF64(); h.k = c; T("F64 F fill null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L fill null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P indexOf null", () => PF64_indexOf(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S indexOf null", () => { SF64_indexOf(c); return 0; }, c); }
{ const h = new FF64_indexOf(); const c: Float64Array = nullF64(); h.k = c; T("F64 F indexOf null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L indexOf null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P includes null", () => PF64_includes(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S includes null", () => { SF64_includes(c); return 0; }, c); }
{ const h = new FF64_includes(); const c: Float64Array = nullF64(); h.k = c; T("F64 F includes null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L includes null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P slice null", () => PF64_slice(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S slice null", () => { SF64_slice(c); return 0; }, c); }
{ const h = new FF64_slice(); const c: Float64Array = nullF64(); h.k = c; T("F64 F slice null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L slice null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P join null", () => PF64_join(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S join null", () => { SF64_join(c); return 0; }, c); }
{ const h = new FF64_join(); const c: Float64Array = nullF64(); h.k = c; T("F64 F join null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L join null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P reverse null", () => PF64_reverse(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S reverse null", () => { SF64_reverse(c); return 0; }, c); }
{ const h = new FF64_reverse(); const c: Float64Array = nullF64(); h.k = c; T("F64 F reverse null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reverse null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P map null", () => PF64_map(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S map null", () => { SF64_map(c); return 0; }, c); }
{ const h = new FF64_map(); const c: Float64Array = nullF64(); h.k = c; T("F64 F map null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L map null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P filter null", () => PF64_filter(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S filter null", () => { SF64_filter(c); return 0; }, c); }
{ const h = new FF64_filter(); const c: Float64Array = nullF64(); h.k = c; T("F64 F filter null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L filter null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P forEach null", () => PF64_forEach(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S forEach null", () => { SF64_forEach(c); return 0; }, c); }
{ const h = new FF64_forEach(); const c: Float64Array = nullF64(); h.k = c; T("F64 F forEach null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L forEach null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P some null", () => PF64_some(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S some null", () => { SF64_some(c); return 0; }, c); }
{ const h = new FF64_some(); const c: Float64Array = nullF64(); h.k = c; T("F64 F some null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L some null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P find null", () => PF64_find(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S find null", () => { SF64_find(c); return 0; }, c); }
{ const h = new FF64_find(); const c: Float64Array = nullF64(); h.k = c; T("F64 F find null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L find null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P reduce null", () => PF64_reduce(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S reduce null", () => { SF64_reduce(c); return 0; }, c); }
{ const h = new FF64_reduce(); const c: Float64Array = nullF64(); h.k = c; T("F64 F reduce null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reduce null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P sort null", () => PF64_sort(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S sort null", () => { SF64_sort(c); return 0; }, c); }
{ const h = new FF64_sort(); const c: Float64Array = nullF64(); h.k = c; T("F64 F sort null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L sort null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P keys null", () => PF64_keys(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S keys null", () => { SF64_keys(c); return 0; }, c); }
{ const h = new FF64_keys(); const c: Float64Array = nullF64(); h.k = c; T("F64 F keys null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L keys null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P values null", () => PF64_values(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S values null", () => { SF64_values(c); return 0; }, c); }
{ const h = new FF64_values(); const c: Float64Array = nullF64(); h.k = c; T("F64 F values null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L values null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P with null", () => PF64_with(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S with null", () => { SF64_with(c); return 0; }, c); }
{ const h = new FF64_with(); const c: Float64Array = nullF64(); h.k = c; T("F64 F with null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L with null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P toSorted null", () => PF64_toSorted(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S toSorted null", () => { SF64_toSorted(c); return 0; }, c); }
{ const h = new FF64_toSorted(); const c: Float64Array = nullF64(); h.k = c; T("F64 F toSorted null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L toSorted null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P set null", () => PF64_set(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S set null", () => { SF64_set(c); return 0; }, c); }
{ const h = new FF64_set(); const c: Float64Array = nullF64(); h.k = c; T("F64 F set null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L set null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = nullF64(); T("F64 P subarray null", () => PF64_subarray(c), c); }
{ const c: Float64Array = nullF64(); T("F64 S subarray null", () => { SF64_subarray(c); return 0; }, c); }
{ const h = new FF64_subarray(); const c: Float64Array = nullF64(); h.k = c; T("F64 F subarray null", () => h.run(), h.k); }
{ const c: Float64Array = nullF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L subarray null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P at undef", () => PF64_at(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S at undef", () => { SF64_at(c); return 0; }, c); }
{ const h = new FF64_at(); const c: Float64Array = undefF64(); h.k = c; T("F64 F at undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L at undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P fill undef", () => PF64_fill(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S fill undef", () => { SF64_fill(c); return 0; }, c); }
{ const h = new FF64_fill(); const c: Float64Array = undefF64(); h.k = c; T("F64 F fill undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L fill undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P indexOf undef", () => PF64_indexOf(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S indexOf undef", () => { SF64_indexOf(c); return 0; }, c); }
{ const h = new FF64_indexOf(); const c: Float64Array = undefF64(); h.k = c; T("F64 F indexOf undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L indexOf undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P includes undef", () => PF64_includes(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S includes undef", () => { SF64_includes(c); return 0; }, c); }
{ const h = new FF64_includes(); const c: Float64Array = undefF64(); h.k = c; T("F64 F includes undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L includes undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P slice undef", () => PF64_slice(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S slice undef", () => { SF64_slice(c); return 0; }, c); }
{ const h = new FF64_slice(); const c: Float64Array = undefF64(); h.k = c; T("F64 F slice undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L slice undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P join undef", () => PF64_join(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S join undef", () => { SF64_join(c); return 0; }, c); }
{ const h = new FF64_join(); const c: Float64Array = undefF64(); h.k = c; T("F64 F join undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L join undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P reverse undef", () => PF64_reverse(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S reverse undef", () => { SF64_reverse(c); return 0; }, c); }
{ const h = new FF64_reverse(); const c: Float64Array = undefF64(); h.k = c; T("F64 F reverse undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reverse undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P map undef", () => PF64_map(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S map undef", () => { SF64_map(c); return 0; }, c); }
{ const h = new FF64_map(); const c: Float64Array = undefF64(); h.k = c; T("F64 F map undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L map undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P filter undef", () => PF64_filter(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S filter undef", () => { SF64_filter(c); return 0; }, c); }
{ const h = new FF64_filter(); const c: Float64Array = undefF64(); h.k = c; T("F64 F filter undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L filter undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P forEach undef", () => PF64_forEach(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S forEach undef", () => { SF64_forEach(c); return 0; }, c); }
{ const h = new FF64_forEach(); const c: Float64Array = undefF64(); h.k = c; T("F64 F forEach undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L forEach undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P some undef", () => PF64_some(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S some undef", () => { SF64_some(c); return 0; }, c); }
{ const h = new FF64_some(); const c: Float64Array = undefF64(); h.k = c; T("F64 F some undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L some undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P find undef", () => PF64_find(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S find undef", () => { SF64_find(c); return 0; }, c); }
{ const h = new FF64_find(); const c: Float64Array = undefF64(); h.k = c; T("F64 F find undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L find undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P reduce undef", () => PF64_reduce(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S reduce undef", () => { SF64_reduce(c); return 0; }, c); }
{ const h = new FF64_reduce(); const c: Float64Array = undefF64(); h.k = c; T("F64 F reduce undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L reduce undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P sort undef", () => PF64_sort(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S sort undef", () => { SF64_sort(c); return 0; }, c); }
{ const h = new FF64_sort(); const c: Float64Array = undefF64(); h.k = c; T("F64 F sort undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L sort undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P keys undef", () => PF64_keys(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S keys undef", () => { SF64_keys(c); return 0; }, c); }
{ const h = new FF64_keys(); const c: Float64Array = undefF64(); h.k = c; T("F64 F keys undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L keys undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P values undef", () => PF64_values(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S values undef", () => { SF64_values(c); return 0; }, c); }
{ const h = new FF64_values(); const c: Float64Array = undefF64(); h.k = c; T("F64 F values undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L values undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P with undef", () => PF64_with(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S with undef", () => { SF64_with(c); return 0; }, c); }
{ const h = new FF64_with(); const c: Float64Array = undefF64(); h.k = c; T("F64 F with undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L with undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P toSorted undef", () => PF64_toSorted(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S toSorted undef", () => { SF64_toSorted(c); return 0; }, c); }
{ const h = new FF64_toSorted(); const c: Float64Array = undefF64(); h.k = c; T("F64 F toSorted undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L toSorted undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P set undef", () => PF64_set(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S set undef", () => { SF64_set(c); return 0; }, c); }
{ const h = new FF64_set(); const c: Float64Array = undefF64(); h.k = c; T("F64 F set undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L set undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Float64Array = undefF64(); T("F64 P subarray undef", () => PF64_subarray(c), c); }
{ const c: Float64Array = undefF64(); T("F64 S subarray undef", () => { SF64_subarray(c); return 0; }, c); }
{ const h = new FF64_subarray(); const c: Float64Array = undefF64(); h.k = c; T("F64 F subarray undef", () => h.run(), h.k); }
{ const c: Float64Array = undefF64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("F64 L subarray undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
function mkI32(): Int32Array { const a = new Int32Array(3); a[0] = 3; a[1] = 1; a[2] = 2; return a; }
class SubI32 extends Int32Array {
  at(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  fill(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  indexOf(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  includes(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  slice(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  join(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  reverse(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  map(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  filter(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  forEach(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  some(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  find(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  reduce(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  sort(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  keys(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  values(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  with(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  toSorted(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  set(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  subarray(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
}
class PlainI32 extends Int32Array { tag(): string { return "plain-sub"; } }
class KI32 { length = 0;
  at(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  fill(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  indexOf(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  includes(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  slice(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  join(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  reverse(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  map(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  filter(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  forEach(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  some(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  find(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  reduce(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  sort(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  keys(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  values(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  with(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  toSorted(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  set(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  subarray(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
}
function subI32(): Int32Array { const s: any = new SubI32(3); s[0] = 3; s[1] = 1; s[2] = 2; return s; }
function psubI32(): Int32Array { const s: any = new PlainI32(3); s[0] = 3; s[1] = 1; s[2] = 2; return s; }
function fakeI32(): Int32Array { const o: any = { length: 0 };
  o.at = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.fill = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.indexOf = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.includes = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.slice = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.join = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.reverse = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.map = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.filter = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.forEach = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.some = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.find = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.reduce = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.sort = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.keys = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.values = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.with = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.toSorted = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.set = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.subarray = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  return o as Int32Array; }
function clsI32(): Int32Array { const o: any = new KI32(); return o as Int32Array; }
function nullI32(): Int32Array { const o: any = JSON.parse("null"); return o as Int32Array; }
function undefI32(): Int32Array { const o: any = JSON.parse("[]")[0]; return o as Int32Array; }
function PI32_at(a: Int32Array): any { return a.at(0); }
function SI32_at(a: Int32Array): void { a.at(0); }
class FI32_at { k: Int32Array = mkI32(); run(): any { return this.k.at(0); } }
function PI32_fill(a: Int32Array): any { return a.fill(5); }
function SI32_fill(a: Int32Array): void { a.fill(5); }
class FI32_fill { k: Int32Array = mkI32(); run(): any { return this.k.fill(5); } }
function PI32_indexOf(a: Int32Array): any { return a.indexOf(1); }
function SI32_indexOf(a: Int32Array): void { a.indexOf(1); }
class FI32_indexOf { k: Int32Array = mkI32(); run(): any { return this.k.indexOf(1); } }
function PI32_includes(a: Int32Array): any { return a.includes(1); }
function SI32_includes(a: Int32Array): void { a.includes(1); }
class FI32_includes { k: Int32Array = mkI32(); run(): any { return this.k.includes(1); } }
function PI32_slice(a: Int32Array): any { return a.slice(0, 2); }
function SI32_slice(a: Int32Array): void { a.slice(0, 2); }
class FI32_slice { k: Int32Array = mkI32(); run(): any { return this.k.slice(0, 2); } }
function PI32_join(a: Int32Array): any { return a.join("-"); }
function SI32_join(a: Int32Array): void { a.join("-"); }
class FI32_join { k: Int32Array = mkI32(); run(): any { return this.k.join("-"); } }
function PI32_reverse(a: Int32Array): any { return a.reverse(); }
function SI32_reverse(a: Int32Array): void { a.reverse(); }
class FI32_reverse { k: Int32Array = mkI32(); run(): any { return this.k.reverse(); } }
function PI32_map(a: Int32Array): any { return a.map((x: any) => x); }
function SI32_map(a: Int32Array): void { a.map((x: any) => x); }
class FI32_map { k: Int32Array = mkI32(); run(): any { return this.k.map((x: any) => x); } }
function PI32_filter(a: Int32Array): any { return a.filter((x: any) => x > 1); }
function SI32_filter(a: Int32Array): void { a.filter((x: any) => x > 1); }
class FI32_filter { k: Int32Array = mkI32(); run(): any { return this.k.filter((x: any) => x > 1); } }
function PI32_forEach(a: Int32Array): any { return a.forEach((x: any) => { sink += Number(x); }); }
function SI32_forEach(a: Int32Array): void { a.forEach((x: any) => { sink += Number(x); }); }
class FI32_forEach { k: Int32Array = mkI32(); run(): any { return this.k.forEach((x: any) => { sink += Number(x); }); } }
function PI32_some(a: Int32Array): any { return a.some((x: any) => x > 1); }
function SI32_some(a: Int32Array): void { a.some((x: any) => x > 1); }
class FI32_some { k: Int32Array = mkI32(); run(): any { return this.k.some((x: any) => x > 1); } }
function PI32_find(a: Int32Array): any { return a.find((x: any) => x > 1); }
function SI32_find(a: Int32Array): void { a.find((x: any) => x > 1); }
class FI32_find { k: Int32Array = mkI32(); run(): any { return this.k.find((x: any) => x > 1); } }
function PI32_reduce(a: Int32Array): any { return a.reduce((p: any, x: any) => p + x, 0); }
function SI32_reduce(a: Int32Array): void { a.reduce((p: any, x: any) => p + x, 0); }
class FI32_reduce { k: Int32Array = mkI32(); run(): any { return this.k.reduce((p: any, x: any) => p + x, 0); } }
function PI32_sort(a: Int32Array): any { return a.sort(); }
function SI32_sort(a: Int32Array): void { a.sort(); }
class FI32_sort { k: Int32Array = mkI32(); run(): any { return this.k.sort(); } }
function PI32_keys(a: Int32Array): any { return a.keys(); }
function SI32_keys(a: Int32Array): void { a.keys(); }
class FI32_keys { k: Int32Array = mkI32(); run(): any { return this.k.keys(); } }
function PI32_values(a: Int32Array): any { return a.values(); }
function SI32_values(a: Int32Array): void { a.values(); }
class FI32_values { k: Int32Array = mkI32(); run(): any { return this.k.values(); } }
function PI32_with(a: Int32Array): any { return a.with(0, 9); }
function SI32_with(a: Int32Array): void { a.with(0, 9); }
class FI32_with { k: Int32Array = mkI32(); run(): any { return this.k.with(0, 9); } }
function PI32_toSorted(a: Int32Array): any { return a.toSorted(); }
function SI32_toSorted(a: Int32Array): void { a.toSorted(); }
class FI32_toSorted { k: Int32Array = mkI32(); run(): any { return this.k.toSorted(); } }
function PI32_set(a: Int32Array): any { return a.set([1]); }
function SI32_set(a: Int32Array): void { a.set([1]); }
class FI32_set { k: Int32Array = mkI32(); run(): any { return this.k.set([1]); } }
function PI32_subarray(a: Int32Array): any { return a.subarray(1); }
function SI32_subarray(a: Int32Array): void { a.subarray(1); }
class FI32_subarray { k: Int32Array = mkI32(); run(): any { return this.k.subarray(1); } }
let GI32: Int32Array = mkI32();
function GI32_at(): any { return GI32.at(0); }
function GI32_fill(): any { return GI32.fill(5); }
function GI32_indexOf(): any { return GI32.indexOf(1); }
function GI32_includes(): any { return GI32.includes(1); }
function GI32_slice(): any { return GI32.slice(0, 2); }
function GI32_join(): any { return GI32.join("-"); }
function GI32_reverse(): any { return GI32.reverse(); }
function GI32_map(): any { return GI32.map((x: any) => x); }
function GI32_filter(): any { return GI32.filter((x: any) => x > 1); }
function GI32_forEach(): any { return GI32.forEach((x: any) => { sink += Number(x); }); }
function GI32_some(): any { return GI32.some((x: any) => x > 1); }
function GI32_find(): any { return GI32.find((x: any) => x > 1); }
function GI32_reduce(): any { return GI32.reduce((p: any, x: any) => p + x, 0); }
function GI32_sort(): any { return GI32.sort(); }
function GI32_keys(): any { return GI32.keys(); }
function GI32_values(): any { return GI32.values(); }
function GI32_with(): any { return GI32.with(0, 9); }
function GI32_toSorted(): any { return GI32.toSorted(); }
function GI32_set(): any { return GI32.set([1]); }
function GI32_subarray(): any { return GI32.subarray(1); }
{ const c: Int32Array = mkI32(); T("I32 P at real", () => PI32_at(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S at real", () => { SI32_at(c); return 0; }, c); }
{ const h = new FI32_at(); const c: Int32Array = mkI32(); h.k = c; T("I32 F at real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G at real", () => GI32_at(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L at real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P fill real", () => PI32_fill(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S fill real", () => { SI32_fill(c); return 0; }, c); }
{ const h = new FI32_fill(); const c: Int32Array = mkI32(); h.k = c; T("I32 F fill real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G fill real", () => GI32_fill(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L fill real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P indexOf real", () => PI32_indexOf(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S indexOf real", () => { SI32_indexOf(c); return 0; }, c); }
{ const h = new FI32_indexOf(); const c: Int32Array = mkI32(); h.k = c; T("I32 F indexOf real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G indexOf real", () => GI32_indexOf(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L indexOf real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P includes real", () => PI32_includes(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S includes real", () => { SI32_includes(c); return 0; }, c); }
{ const h = new FI32_includes(); const c: Int32Array = mkI32(); h.k = c; T("I32 F includes real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G includes real", () => GI32_includes(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L includes real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P slice real", () => PI32_slice(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S slice real", () => { SI32_slice(c); return 0; }, c); }
{ const h = new FI32_slice(); const c: Int32Array = mkI32(); h.k = c; T("I32 F slice real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G slice real", () => GI32_slice(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L slice real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P join real", () => PI32_join(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S join real", () => { SI32_join(c); return 0; }, c); }
{ const h = new FI32_join(); const c: Int32Array = mkI32(); h.k = c; T("I32 F join real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G join real", () => GI32_join(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L join real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P reverse real", () => PI32_reverse(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S reverse real", () => { SI32_reverse(c); return 0; }, c); }
{ const h = new FI32_reverse(); const c: Int32Array = mkI32(); h.k = c; T("I32 F reverse real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G reverse real", () => GI32_reverse(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reverse real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P map real", () => PI32_map(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S map real", () => { SI32_map(c); return 0; }, c); }
{ const h = new FI32_map(); const c: Int32Array = mkI32(); h.k = c; T("I32 F map real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G map real", () => GI32_map(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L map real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P filter real", () => PI32_filter(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S filter real", () => { SI32_filter(c); return 0; }, c); }
{ const h = new FI32_filter(); const c: Int32Array = mkI32(); h.k = c; T("I32 F filter real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G filter real", () => GI32_filter(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L filter real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P forEach real", () => PI32_forEach(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S forEach real", () => { SI32_forEach(c); return 0; }, c); }
{ const h = new FI32_forEach(); const c: Int32Array = mkI32(); h.k = c; T("I32 F forEach real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G forEach real", () => GI32_forEach(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L forEach real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P some real", () => PI32_some(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S some real", () => { SI32_some(c); return 0; }, c); }
{ const h = new FI32_some(); const c: Int32Array = mkI32(); h.k = c; T("I32 F some real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G some real", () => GI32_some(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L some real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P find real", () => PI32_find(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S find real", () => { SI32_find(c); return 0; }, c); }
{ const h = new FI32_find(); const c: Int32Array = mkI32(); h.k = c; T("I32 F find real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G find real", () => GI32_find(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L find real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P reduce real", () => PI32_reduce(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S reduce real", () => { SI32_reduce(c); return 0; }, c); }
{ const h = new FI32_reduce(); const c: Int32Array = mkI32(); h.k = c; T("I32 F reduce real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G reduce real", () => GI32_reduce(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reduce real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P sort real", () => PI32_sort(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S sort real", () => { SI32_sort(c); return 0; }, c); }
{ const h = new FI32_sort(); const c: Int32Array = mkI32(); h.k = c; T("I32 F sort real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G sort real", () => GI32_sort(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L sort real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P keys real", () => PI32_keys(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S keys real", () => { SI32_keys(c); return 0; }, c); }
{ const h = new FI32_keys(); const c: Int32Array = mkI32(); h.k = c; T("I32 F keys real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G keys real", () => GI32_keys(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L keys real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P values real", () => PI32_values(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S values real", () => { SI32_values(c); return 0; }, c); }
{ const h = new FI32_values(); const c: Int32Array = mkI32(); h.k = c; T("I32 F values real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G values real", () => GI32_values(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L values real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P with real", () => PI32_with(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S with real", () => { SI32_with(c); return 0; }, c); }
{ const h = new FI32_with(); const c: Int32Array = mkI32(); h.k = c; T("I32 F with real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G with real", () => GI32_with(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L with real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P toSorted real", () => PI32_toSorted(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S toSorted real", () => { SI32_toSorted(c); return 0; }, c); }
{ const h = new FI32_toSorted(); const c: Int32Array = mkI32(); h.k = c; T("I32 F toSorted real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G toSorted real", () => GI32_toSorted(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L toSorted real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P set real", () => PI32_set(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S set real", () => { SI32_set(c); return 0; }, c); }
{ const h = new FI32_set(); const c: Int32Array = mkI32(); h.k = c; T("I32 F set real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G set real", () => GI32_set(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L set real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = mkI32(); T("I32 P subarray real", () => PI32_subarray(c), c); }
{ const c: Int32Array = mkI32(); T("I32 S subarray real", () => { SI32_subarray(c); return 0; }, c); }
{ const h = new FI32_subarray(); const c: Int32Array = mkI32(); h.k = c; T("I32 F subarray real", () => h.run(), h.k); }
{ GI32 = mkI32(); T("I32 G subarray real", () => GI32_subarray(), GI32); }
{ const c: Int32Array = mkI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L subarray real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P at fake", () => PI32_at(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S at fake", () => { SI32_at(c); return 0; }, c); }
{ const h = new FI32_at(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F at fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G at fake", () => GI32_at(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L at fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P fill fake", () => PI32_fill(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S fill fake", () => { SI32_fill(c); return 0; }, c); }
{ const h = new FI32_fill(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F fill fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G fill fake", () => GI32_fill(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L fill fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P indexOf fake", () => PI32_indexOf(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S indexOf fake", () => { SI32_indexOf(c); return 0; }, c); }
{ const h = new FI32_indexOf(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F indexOf fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G indexOf fake", () => GI32_indexOf(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L indexOf fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P includes fake", () => PI32_includes(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S includes fake", () => { SI32_includes(c); return 0; }, c); }
{ const h = new FI32_includes(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F includes fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G includes fake", () => GI32_includes(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L includes fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P slice fake", () => PI32_slice(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S slice fake", () => { SI32_slice(c); return 0; }, c); }
{ const h = new FI32_slice(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F slice fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G slice fake", () => GI32_slice(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L slice fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P join fake", () => PI32_join(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S join fake", () => { SI32_join(c); return 0; }, c); }
{ const h = new FI32_join(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F join fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G join fake", () => GI32_join(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L join fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P reverse fake", () => PI32_reverse(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S reverse fake", () => { SI32_reverse(c); return 0; }, c); }
{ const h = new FI32_reverse(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F reverse fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G reverse fake", () => GI32_reverse(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reverse fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P map fake", () => PI32_map(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S map fake", () => { SI32_map(c); return 0; }, c); }
{ const h = new FI32_map(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F map fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G map fake", () => GI32_map(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L map fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P filter fake", () => PI32_filter(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S filter fake", () => { SI32_filter(c); return 0; }, c); }
{ const h = new FI32_filter(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F filter fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G filter fake", () => GI32_filter(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L filter fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P forEach fake", () => PI32_forEach(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S forEach fake", () => { SI32_forEach(c); return 0; }, c); }
{ const h = new FI32_forEach(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F forEach fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G forEach fake", () => GI32_forEach(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L forEach fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P some fake", () => PI32_some(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S some fake", () => { SI32_some(c); return 0; }, c); }
{ const h = new FI32_some(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F some fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G some fake", () => GI32_some(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L some fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P find fake", () => PI32_find(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S find fake", () => { SI32_find(c); return 0; }, c); }
{ const h = new FI32_find(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F find fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G find fake", () => GI32_find(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L find fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P reduce fake", () => PI32_reduce(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S reduce fake", () => { SI32_reduce(c); return 0; }, c); }
{ const h = new FI32_reduce(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F reduce fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G reduce fake", () => GI32_reduce(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reduce fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P sort fake", () => PI32_sort(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S sort fake", () => { SI32_sort(c); return 0; }, c); }
{ const h = new FI32_sort(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F sort fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G sort fake", () => GI32_sort(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L sort fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P keys fake", () => PI32_keys(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S keys fake", () => { SI32_keys(c); return 0; }, c); }
{ const h = new FI32_keys(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F keys fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G keys fake", () => GI32_keys(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L keys fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P values fake", () => PI32_values(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S values fake", () => { SI32_values(c); return 0; }, c); }
{ const h = new FI32_values(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F values fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G values fake", () => GI32_values(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L values fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P with fake", () => PI32_with(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S with fake", () => { SI32_with(c); return 0; }, c); }
{ const h = new FI32_with(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F with fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G with fake", () => GI32_with(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L with fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P toSorted fake", () => PI32_toSorted(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S toSorted fake", () => { SI32_toSorted(c); return 0; }, c); }
{ const h = new FI32_toSorted(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F toSorted fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G toSorted fake", () => GI32_toSorted(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L toSorted fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P set fake", () => PI32_set(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S set fake", () => { SI32_set(c); return 0; }, c); }
{ const h = new FI32_set(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F set fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G set fake", () => GI32_set(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L set fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = fakeI32(); T("I32 P subarray fake", () => PI32_subarray(c), c); }
{ const c: Int32Array = fakeI32(); T("I32 S subarray fake", () => { SI32_subarray(c); return 0; }, c); }
{ const h = new FI32_subarray(); const c: Int32Array = fakeI32(); h.k = c; T("I32 F subarray fake", () => h.run(), h.k); }
{ GI32 = fakeI32(); T("I32 G subarray fake", () => GI32_subarray(), GI32); }
{ const c: Int32Array = fakeI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L subarray fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P at cls", () => PI32_at(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S at cls", () => { SI32_at(c); return 0; }, c); }
{ const h = new FI32_at(); const c: Int32Array = clsI32(); h.k = c; T("I32 F at cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G at cls", () => GI32_at(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L at cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P fill cls", () => PI32_fill(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S fill cls", () => { SI32_fill(c); return 0; }, c); }
{ const h = new FI32_fill(); const c: Int32Array = clsI32(); h.k = c; T("I32 F fill cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G fill cls", () => GI32_fill(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L fill cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P indexOf cls", () => PI32_indexOf(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S indexOf cls", () => { SI32_indexOf(c); return 0; }, c); }
{ const h = new FI32_indexOf(); const c: Int32Array = clsI32(); h.k = c; T("I32 F indexOf cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G indexOf cls", () => GI32_indexOf(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L indexOf cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P includes cls", () => PI32_includes(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S includes cls", () => { SI32_includes(c); return 0; }, c); }
{ const h = new FI32_includes(); const c: Int32Array = clsI32(); h.k = c; T("I32 F includes cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G includes cls", () => GI32_includes(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L includes cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P slice cls", () => PI32_slice(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S slice cls", () => { SI32_slice(c); return 0; }, c); }
{ const h = new FI32_slice(); const c: Int32Array = clsI32(); h.k = c; T("I32 F slice cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G slice cls", () => GI32_slice(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L slice cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P join cls", () => PI32_join(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S join cls", () => { SI32_join(c); return 0; }, c); }
{ const h = new FI32_join(); const c: Int32Array = clsI32(); h.k = c; T("I32 F join cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G join cls", () => GI32_join(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L join cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P reverse cls", () => PI32_reverse(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S reverse cls", () => { SI32_reverse(c); return 0; }, c); }
{ const h = new FI32_reverse(); const c: Int32Array = clsI32(); h.k = c; T("I32 F reverse cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G reverse cls", () => GI32_reverse(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reverse cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P map cls", () => PI32_map(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S map cls", () => { SI32_map(c); return 0; }, c); }
{ const h = new FI32_map(); const c: Int32Array = clsI32(); h.k = c; T("I32 F map cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G map cls", () => GI32_map(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L map cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P filter cls", () => PI32_filter(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S filter cls", () => { SI32_filter(c); return 0; }, c); }
{ const h = new FI32_filter(); const c: Int32Array = clsI32(); h.k = c; T("I32 F filter cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G filter cls", () => GI32_filter(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L filter cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P forEach cls", () => PI32_forEach(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S forEach cls", () => { SI32_forEach(c); return 0; }, c); }
{ const h = new FI32_forEach(); const c: Int32Array = clsI32(); h.k = c; T("I32 F forEach cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G forEach cls", () => GI32_forEach(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L forEach cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P some cls", () => PI32_some(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S some cls", () => { SI32_some(c); return 0; }, c); }
{ const h = new FI32_some(); const c: Int32Array = clsI32(); h.k = c; T("I32 F some cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G some cls", () => GI32_some(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L some cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P find cls", () => PI32_find(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S find cls", () => { SI32_find(c); return 0; }, c); }
{ const h = new FI32_find(); const c: Int32Array = clsI32(); h.k = c; T("I32 F find cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G find cls", () => GI32_find(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L find cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P reduce cls", () => PI32_reduce(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S reduce cls", () => { SI32_reduce(c); return 0; }, c); }
{ const h = new FI32_reduce(); const c: Int32Array = clsI32(); h.k = c; T("I32 F reduce cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G reduce cls", () => GI32_reduce(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reduce cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P sort cls", () => PI32_sort(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S sort cls", () => { SI32_sort(c); return 0; }, c); }
{ const h = new FI32_sort(); const c: Int32Array = clsI32(); h.k = c; T("I32 F sort cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G sort cls", () => GI32_sort(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L sort cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P keys cls", () => PI32_keys(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S keys cls", () => { SI32_keys(c); return 0; }, c); }
{ const h = new FI32_keys(); const c: Int32Array = clsI32(); h.k = c; T("I32 F keys cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G keys cls", () => GI32_keys(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L keys cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P values cls", () => PI32_values(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S values cls", () => { SI32_values(c); return 0; }, c); }
{ const h = new FI32_values(); const c: Int32Array = clsI32(); h.k = c; T("I32 F values cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G values cls", () => GI32_values(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L values cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P with cls", () => PI32_with(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S with cls", () => { SI32_with(c); return 0; }, c); }
{ const h = new FI32_with(); const c: Int32Array = clsI32(); h.k = c; T("I32 F with cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G with cls", () => GI32_with(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L with cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P toSorted cls", () => PI32_toSorted(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S toSorted cls", () => { SI32_toSorted(c); return 0; }, c); }
{ const h = new FI32_toSorted(); const c: Int32Array = clsI32(); h.k = c; T("I32 F toSorted cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G toSorted cls", () => GI32_toSorted(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L toSorted cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P set cls", () => PI32_set(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S set cls", () => { SI32_set(c); return 0; }, c); }
{ const h = new FI32_set(); const c: Int32Array = clsI32(); h.k = c; T("I32 F set cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G set cls", () => GI32_set(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L set cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = clsI32(); T("I32 P subarray cls", () => PI32_subarray(c), c); }
{ const c: Int32Array = clsI32(); T("I32 S subarray cls", () => { SI32_subarray(c); return 0; }, c); }
{ const h = new FI32_subarray(); const c: Int32Array = clsI32(); h.k = c; T("I32 F subarray cls", () => h.run(), h.k); }
{ GI32 = clsI32(); T("I32 G subarray cls", () => GI32_subarray(), GI32); }
{ const c: Int32Array = clsI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L subarray cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P at sub", () => PI32_at(c), c); }
{ const c: Int32Array = subI32(); T("I32 S at sub", () => { SI32_at(c); return 0; }, c); }
{ const h = new FI32_at(); const c: Int32Array = subI32(); h.k = c; T("I32 F at sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L at sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P fill sub", () => PI32_fill(c), c); }
{ const c: Int32Array = subI32(); T("I32 S fill sub", () => { SI32_fill(c); return 0; }, c); }
{ const h = new FI32_fill(); const c: Int32Array = subI32(); h.k = c; T("I32 F fill sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L fill sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P indexOf sub", () => PI32_indexOf(c), c); }
{ const c: Int32Array = subI32(); T("I32 S indexOf sub", () => { SI32_indexOf(c); return 0; }, c); }
{ const h = new FI32_indexOf(); const c: Int32Array = subI32(); h.k = c; T("I32 F indexOf sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L indexOf sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P includes sub", () => PI32_includes(c), c); }
{ const c: Int32Array = subI32(); T("I32 S includes sub", () => { SI32_includes(c); return 0; }, c); }
{ const h = new FI32_includes(); const c: Int32Array = subI32(); h.k = c; T("I32 F includes sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L includes sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P slice sub", () => PI32_slice(c), c); }
{ const c: Int32Array = subI32(); T("I32 S slice sub", () => { SI32_slice(c); return 0; }, c); }
{ const h = new FI32_slice(); const c: Int32Array = subI32(); h.k = c; T("I32 F slice sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L slice sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P join sub", () => PI32_join(c), c); }
{ const c: Int32Array = subI32(); T("I32 S join sub", () => { SI32_join(c); return 0; }, c); }
{ const h = new FI32_join(); const c: Int32Array = subI32(); h.k = c; T("I32 F join sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L join sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P reverse sub", () => PI32_reverse(c), c); }
{ const c: Int32Array = subI32(); T("I32 S reverse sub", () => { SI32_reverse(c); return 0; }, c); }
{ const h = new FI32_reverse(); const c: Int32Array = subI32(); h.k = c; T("I32 F reverse sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reverse sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P map sub", () => PI32_map(c), c); }
{ const c: Int32Array = subI32(); T("I32 S map sub", () => { SI32_map(c); return 0; }, c); }
{ const h = new FI32_map(); const c: Int32Array = subI32(); h.k = c; T("I32 F map sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L map sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P filter sub", () => PI32_filter(c), c); }
{ const c: Int32Array = subI32(); T("I32 S filter sub", () => { SI32_filter(c); return 0; }, c); }
{ const h = new FI32_filter(); const c: Int32Array = subI32(); h.k = c; T("I32 F filter sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L filter sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P forEach sub", () => PI32_forEach(c), c); }
{ const c: Int32Array = subI32(); T("I32 S forEach sub", () => { SI32_forEach(c); return 0; }, c); }
{ const h = new FI32_forEach(); const c: Int32Array = subI32(); h.k = c; T("I32 F forEach sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L forEach sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P some sub", () => PI32_some(c), c); }
{ const c: Int32Array = subI32(); T("I32 S some sub", () => { SI32_some(c); return 0; }, c); }
{ const h = new FI32_some(); const c: Int32Array = subI32(); h.k = c; T("I32 F some sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L some sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P find sub", () => PI32_find(c), c); }
{ const c: Int32Array = subI32(); T("I32 S find sub", () => { SI32_find(c); return 0; }, c); }
{ const h = new FI32_find(); const c: Int32Array = subI32(); h.k = c; T("I32 F find sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L find sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P reduce sub", () => PI32_reduce(c), c); }
{ const c: Int32Array = subI32(); T("I32 S reduce sub", () => { SI32_reduce(c); return 0; }, c); }
{ const h = new FI32_reduce(); const c: Int32Array = subI32(); h.k = c; T("I32 F reduce sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reduce sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P sort sub", () => PI32_sort(c), c); }
{ const c: Int32Array = subI32(); T("I32 S sort sub", () => { SI32_sort(c); return 0; }, c); }
{ const h = new FI32_sort(); const c: Int32Array = subI32(); h.k = c; T("I32 F sort sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L sort sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P keys sub", () => PI32_keys(c), c); }
{ const c: Int32Array = subI32(); T("I32 S keys sub", () => { SI32_keys(c); return 0; }, c); }
{ const h = new FI32_keys(); const c: Int32Array = subI32(); h.k = c; T("I32 F keys sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L keys sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P values sub", () => PI32_values(c), c); }
{ const c: Int32Array = subI32(); T("I32 S values sub", () => { SI32_values(c); return 0; }, c); }
{ const h = new FI32_values(); const c: Int32Array = subI32(); h.k = c; T("I32 F values sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L values sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P with sub", () => PI32_with(c), c); }
{ const c: Int32Array = subI32(); T("I32 S with sub", () => { SI32_with(c); return 0; }, c); }
{ const h = new FI32_with(); const c: Int32Array = subI32(); h.k = c; T("I32 F with sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L with sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P toSorted sub", () => PI32_toSorted(c), c); }
{ const c: Int32Array = subI32(); T("I32 S toSorted sub", () => { SI32_toSorted(c); return 0; }, c); }
{ const h = new FI32_toSorted(); const c: Int32Array = subI32(); h.k = c; T("I32 F toSorted sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L toSorted sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P set sub", () => PI32_set(c), c); }
{ const c: Int32Array = subI32(); T("I32 S set sub", () => { SI32_set(c); return 0; }, c); }
{ const h = new FI32_set(); const c: Int32Array = subI32(); h.k = c; T("I32 F set sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L set sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = subI32(); T("I32 P subarray sub", () => PI32_subarray(c), c); }
{ const c: Int32Array = subI32(); T("I32 S subarray sub", () => { SI32_subarray(c); return 0; }, c); }
{ const h = new FI32_subarray(); const c: Int32Array = subI32(); h.k = c; T("I32 F subarray sub", () => h.run(), h.k); }
{ const c: Int32Array = subI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L subarray sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P at psub", () => PI32_at(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S at psub", () => { SI32_at(c); return 0; }, c); }
{ const h = new FI32_at(); const c: Int32Array = psubI32(); h.k = c; T("I32 F at psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G at psub", () => GI32_at(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L at psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P fill psub", () => PI32_fill(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S fill psub", () => { SI32_fill(c); return 0; }, c); }
{ const h = new FI32_fill(); const c: Int32Array = psubI32(); h.k = c; T("I32 F fill psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G fill psub", () => GI32_fill(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L fill psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P indexOf psub", () => PI32_indexOf(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S indexOf psub", () => { SI32_indexOf(c); return 0; }, c); }
{ const h = new FI32_indexOf(); const c: Int32Array = psubI32(); h.k = c; T("I32 F indexOf psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G indexOf psub", () => GI32_indexOf(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L indexOf psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P includes psub", () => PI32_includes(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S includes psub", () => { SI32_includes(c); return 0; }, c); }
{ const h = new FI32_includes(); const c: Int32Array = psubI32(); h.k = c; T("I32 F includes psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G includes psub", () => GI32_includes(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L includes psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P slice psub", () => PI32_slice(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S slice psub", () => { SI32_slice(c); return 0; }, c); }
{ const h = new FI32_slice(); const c: Int32Array = psubI32(); h.k = c; T("I32 F slice psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G slice psub", () => GI32_slice(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L slice psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P join psub", () => PI32_join(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S join psub", () => { SI32_join(c); return 0; }, c); }
{ const h = new FI32_join(); const c: Int32Array = psubI32(); h.k = c; T("I32 F join psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G join psub", () => GI32_join(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L join psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P reverse psub", () => PI32_reverse(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S reverse psub", () => { SI32_reverse(c); return 0; }, c); }
{ const h = new FI32_reverse(); const c: Int32Array = psubI32(); h.k = c; T("I32 F reverse psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G reverse psub", () => GI32_reverse(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reverse psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P map psub", () => PI32_map(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S map psub", () => { SI32_map(c); return 0; }, c); }
{ const h = new FI32_map(); const c: Int32Array = psubI32(); h.k = c; T("I32 F map psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G map psub", () => GI32_map(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L map psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P filter psub", () => PI32_filter(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S filter psub", () => { SI32_filter(c); return 0; }, c); }
{ const h = new FI32_filter(); const c: Int32Array = psubI32(); h.k = c; T("I32 F filter psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G filter psub", () => GI32_filter(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L filter psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P forEach psub", () => PI32_forEach(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S forEach psub", () => { SI32_forEach(c); return 0; }, c); }
{ const h = new FI32_forEach(); const c: Int32Array = psubI32(); h.k = c; T("I32 F forEach psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G forEach psub", () => GI32_forEach(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L forEach psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P some psub", () => PI32_some(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S some psub", () => { SI32_some(c); return 0; }, c); }
{ const h = new FI32_some(); const c: Int32Array = psubI32(); h.k = c; T("I32 F some psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G some psub", () => GI32_some(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L some psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P find psub", () => PI32_find(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S find psub", () => { SI32_find(c); return 0; }, c); }
{ const h = new FI32_find(); const c: Int32Array = psubI32(); h.k = c; T("I32 F find psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G find psub", () => GI32_find(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L find psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P reduce psub", () => PI32_reduce(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S reduce psub", () => { SI32_reduce(c); return 0; }, c); }
{ const h = new FI32_reduce(); const c: Int32Array = psubI32(); h.k = c; T("I32 F reduce psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G reduce psub", () => GI32_reduce(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reduce psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P sort psub", () => PI32_sort(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S sort psub", () => { SI32_sort(c); return 0; }, c); }
{ const h = new FI32_sort(); const c: Int32Array = psubI32(); h.k = c; T("I32 F sort psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G sort psub", () => GI32_sort(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L sort psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P keys psub", () => PI32_keys(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S keys psub", () => { SI32_keys(c); return 0; }, c); }
{ const h = new FI32_keys(); const c: Int32Array = psubI32(); h.k = c; T("I32 F keys psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G keys psub", () => GI32_keys(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L keys psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P values psub", () => PI32_values(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S values psub", () => { SI32_values(c); return 0; }, c); }
{ const h = new FI32_values(); const c: Int32Array = psubI32(); h.k = c; T("I32 F values psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G values psub", () => GI32_values(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L values psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P with psub", () => PI32_with(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S with psub", () => { SI32_with(c); return 0; }, c); }
{ const h = new FI32_with(); const c: Int32Array = psubI32(); h.k = c; T("I32 F with psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G with psub", () => GI32_with(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L with psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P toSorted psub", () => PI32_toSorted(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S toSorted psub", () => { SI32_toSorted(c); return 0; }, c); }
{ const h = new FI32_toSorted(); const c: Int32Array = psubI32(); h.k = c; T("I32 F toSorted psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G toSorted psub", () => GI32_toSorted(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L toSorted psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P set psub", () => PI32_set(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S set psub", () => { SI32_set(c); return 0; }, c); }
{ const h = new FI32_set(); const c: Int32Array = psubI32(); h.k = c; T("I32 F set psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G set psub", () => GI32_set(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L set psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = psubI32(); T("I32 P subarray psub", () => PI32_subarray(c), c); }
{ const c: Int32Array = psubI32(); T("I32 S subarray psub", () => { SI32_subarray(c); return 0; }, c); }
{ const h = new FI32_subarray(); const c: Int32Array = psubI32(); h.k = c; T("I32 F subarray psub", () => h.run(), h.k); }
{ GI32 = psubI32(); T("I32 G subarray psub", () => GI32_subarray(), GI32); }
{ const c: Int32Array = psubI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L subarray psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P at null", () => PI32_at(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S at null", () => { SI32_at(c); return 0; }, c); }
{ const h = new FI32_at(); const c: Int32Array = nullI32(); h.k = c; T("I32 F at null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L at null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P fill null", () => PI32_fill(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S fill null", () => { SI32_fill(c); return 0; }, c); }
{ const h = new FI32_fill(); const c: Int32Array = nullI32(); h.k = c; T("I32 F fill null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L fill null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P indexOf null", () => PI32_indexOf(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S indexOf null", () => { SI32_indexOf(c); return 0; }, c); }
{ const h = new FI32_indexOf(); const c: Int32Array = nullI32(); h.k = c; T("I32 F indexOf null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L indexOf null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P includes null", () => PI32_includes(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S includes null", () => { SI32_includes(c); return 0; }, c); }
{ const h = new FI32_includes(); const c: Int32Array = nullI32(); h.k = c; T("I32 F includes null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L includes null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P slice null", () => PI32_slice(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S slice null", () => { SI32_slice(c); return 0; }, c); }
{ const h = new FI32_slice(); const c: Int32Array = nullI32(); h.k = c; T("I32 F slice null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L slice null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P join null", () => PI32_join(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S join null", () => { SI32_join(c); return 0; }, c); }
{ const h = new FI32_join(); const c: Int32Array = nullI32(); h.k = c; T("I32 F join null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L join null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P reverse null", () => PI32_reverse(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S reverse null", () => { SI32_reverse(c); return 0; }, c); }
{ const h = new FI32_reverse(); const c: Int32Array = nullI32(); h.k = c; T("I32 F reverse null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reverse null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P map null", () => PI32_map(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S map null", () => { SI32_map(c); return 0; }, c); }
{ const h = new FI32_map(); const c: Int32Array = nullI32(); h.k = c; T("I32 F map null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L map null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P filter null", () => PI32_filter(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S filter null", () => { SI32_filter(c); return 0; }, c); }
{ const h = new FI32_filter(); const c: Int32Array = nullI32(); h.k = c; T("I32 F filter null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L filter null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P forEach null", () => PI32_forEach(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S forEach null", () => { SI32_forEach(c); return 0; }, c); }
{ const h = new FI32_forEach(); const c: Int32Array = nullI32(); h.k = c; T("I32 F forEach null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L forEach null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P some null", () => PI32_some(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S some null", () => { SI32_some(c); return 0; }, c); }
{ const h = new FI32_some(); const c: Int32Array = nullI32(); h.k = c; T("I32 F some null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L some null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P find null", () => PI32_find(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S find null", () => { SI32_find(c); return 0; }, c); }
{ const h = new FI32_find(); const c: Int32Array = nullI32(); h.k = c; T("I32 F find null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L find null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P reduce null", () => PI32_reduce(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S reduce null", () => { SI32_reduce(c); return 0; }, c); }
{ const h = new FI32_reduce(); const c: Int32Array = nullI32(); h.k = c; T("I32 F reduce null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reduce null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P sort null", () => PI32_sort(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S sort null", () => { SI32_sort(c); return 0; }, c); }
{ const h = new FI32_sort(); const c: Int32Array = nullI32(); h.k = c; T("I32 F sort null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L sort null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P keys null", () => PI32_keys(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S keys null", () => { SI32_keys(c); return 0; }, c); }
{ const h = new FI32_keys(); const c: Int32Array = nullI32(); h.k = c; T("I32 F keys null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L keys null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P values null", () => PI32_values(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S values null", () => { SI32_values(c); return 0; }, c); }
{ const h = new FI32_values(); const c: Int32Array = nullI32(); h.k = c; T("I32 F values null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L values null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P with null", () => PI32_with(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S with null", () => { SI32_with(c); return 0; }, c); }
{ const h = new FI32_with(); const c: Int32Array = nullI32(); h.k = c; T("I32 F with null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L with null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P toSorted null", () => PI32_toSorted(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S toSorted null", () => { SI32_toSorted(c); return 0; }, c); }
{ const h = new FI32_toSorted(); const c: Int32Array = nullI32(); h.k = c; T("I32 F toSorted null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L toSorted null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P set null", () => PI32_set(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S set null", () => { SI32_set(c); return 0; }, c); }
{ const h = new FI32_set(); const c: Int32Array = nullI32(); h.k = c; T("I32 F set null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L set null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = nullI32(); T("I32 P subarray null", () => PI32_subarray(c), c); }
{ const c: Int32Array = nullI32(); T("I32 S subarray null", () => { SI32_subarray(c); return 0; }, c); }
{ const h = new FI32_subarray(); const c: Int32Array = nullI32(); h.k = c; T("I32 F subarray null", () => h.run(), h.k); }
{ const c: Int32Array = nullI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L subarray null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P at undef", () => PI32_at(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S at undef", () => { SI32_at(c); return 0; }, c); }
{ const h = new FI32_at(); const c: Int32Array = undefI32(); h.k = c; T("I32 F at undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L at undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P fill undef", () => PI32_fill(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S fill undef", () => { SI32_fill(c); return 0; }, c); }
{ const h = new FI32_fill(); const c: Int32Array = undefI32(); h.k = c; T("I32 F fill undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L fill undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P indexOf undef", () => PI32_indexOf(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S indexOf undef", () => { SI32_indexOf(c); return 0; }, c); }
{ const h = new FI32_indexOf(); const c: Int32Array = undefI32(); h.k = c; T("I32 F indexOf undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L indexOf undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P includes undef", () => PI32_includes(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S includes undef", () => { SI32_includes(c); return 0; }, c); }
{ const h = new FI32_includes(); const c: Int32Array = undefI32(); h.k = c; T("I32 F includes undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L includes undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P slice undef", () => PI32_slice(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S slice undef", () => { SI32_slice(c); return 0; }, c); }
{ const h = new FI32_slice(); const c: Int32Array = undefI32(); h.k = c; T("I32 F slice undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L slice undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P join undef", () => PI32_join(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S join undef", () => { SI32_join(c); return 0; }, c); }
{ const h = new FI32_join(); const c: Int32Array = undefI32(); h.k = c; T("I32 F join undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L join undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P reverse undef", () => PI32_reverse(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S reverse undef", () => { SI32_reverse(c); return 0; }, c); }
{ const h = new FI32_reverse(); const c: Int32Array = undefI32(); h.k = c; T("I32 F reverse undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reverse undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P map undef", () => PI32_map(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S map undef", () => { SI32_map(c); return 0; }, c); }
{ const h = new FI32_map(); const c: Int32Array = undefI32(); h.k = c; T("I32 F map undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L map undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P filter undef", () => PI32_filter(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S filter undef", () => { SI32_filter(c); return 0; }, c); }
{ const h = new FI32_filter(); const c: Int32Array = undefI32(); h.k = c; T("I32 F filter undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L filter undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P forEach undef", () => PI32_forEach(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S forEach undef", () => { SI32_forEach(c); return 0; }, c); }
{ const h = new FI32_forEach(); const c: Int32Array = undefI32(); h.k = c; T("I32 F forEach undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L forEach undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P some undef", () => PI32_some(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S some undef", () => { SI32_some(c); return 0; }, c); }
{ const h = new FI32_some(); const c: Int32Array = undefI32(); h.k = c; T("I32 F some undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L some undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P find undef", () => PI32_find(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S find undef", () => { SI32_find(c); return 0; }, c); }
{ const h = new FI32_find(); const c: Int32Array = undefI32(); h.k = c; T("I32 F find undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L find undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P reduce undef", () => PI32_reduce(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S reduce undef", () => { SI32_reduce(c); return 0; }, c); }
{ const h = new FI32_reduce(); const c: Int32Array = undefI32(); h.k = c; T("I32 F reduce undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L reduce undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P sort undef", () => PI32_sort(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S sort undef", () => { SI32_sort(c); return 0; }, c); }
{ const h = new FI32_sort(); const c: Int32Array = undefI32(); h.k = c; T("I32 F sort undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L sort undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P keys undef", () => PI32_keys(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S keys undef", () => { SI32_keys(c); return 0; }, c); }
{ const h = new FI32_keys(); const c: Int32Array = undefI32(); h.k = c; T("I32 F keys undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L keys undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P values undef", () => PI32_values(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S values undef", () => { SI32_values(c); return 0; }, c); }
{ const h = new FI32_values(); const c: Int32Array = undefI32(); h.k = c; T("I32 F values undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L values undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P with undef", () => PI32_with(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S with undef", () => { SI32_with(c); return 0; }, c); }
{ const h = new FI32_with(); const c: Int32Array = undefI32(); h.k = c; T("I32 F with undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L with undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P toSorted undef", () => PI32_toSorted(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S toSorted undef", () => { SI32_toSorted(c); return 0; }, c); }
{ const h = new FI32_toSorted(); const c: Int32Array = undefI32(); h.k = c; T("I32 F toSorted undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L toSorted undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P set undef", () => PI32_set(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S set undef", () => { SI32_set(c); return 0; }, c); }
{ const h = new FI32_set(); const c: Int32Array = undefI32(); h.k = c; T("I32 F set undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L set undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: Int32Array = undefI32(); T("I32 P subarray undef", () => PI32_subarray(c), c); }
{ const c: Int32Array = undefI32(); T("I32 S subarray undef", () => { SI32_subarray(c); return 0; }, c); }
{ const h = new FI32_subarray(); const c: Int32Array = undefI32(); h.k = c; T("I32 F subarray undef", () => h.run(), h.k); }
{ const c: Int32Array = undefI32(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("I32 L subarray undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
function mkB64(): BigInt64Array { const a = new BigInt64Array(3); a[0] = 3n; a[1] = 1n; a[2] = 2n; return a; }
class SubB64 extends BigInt64Array {
  at(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  fill(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  indexOf(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  includes(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  slice(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  join(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  reverse(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  map(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  filter(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  forEach(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  some(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  find(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  reduce(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  sort(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  keys(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  values(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  with(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  toSorted(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  set(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
  subarray(...args: any[]): any { calls++; return "SUB(" + args.length + ")"; }
}
class PlainB64 extends BigInt64Array { tag(): string { return "plain-sub"; } }
class KB64 { length = 0;
  at(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  fill(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  indexOf(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  includes(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  slice(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  join(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  reverse(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  map(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  filter(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  forEach(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  some(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  find(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  reduce(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  sort(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  keys(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  values(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  with(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  toSorted(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  set(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
  subarray(...args: any[]): any { calls++; return "CLS(" + args.length + ")"; }
}
function subB64(): BigInt64Array { const s: any = new SubB64(3); s[0] = 3n; s[1] = 1n; s[2] = 2n; return s; }
function psubB64(): BigInt64Array { const s: any = new PlainB64(3); s[0] = 3n; s[1] = 1n; s[2] = 2n; return s; }
function fakeB64(): BigInt64Array { const o: any = { length: 0 };
  o.at = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.fill = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.indexOf = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.includes = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.slice = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.join = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.reverse = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.map = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.filter = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.forEach = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.some = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.find = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.reduce = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.sort = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.keys = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.values = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.with = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.toSorted = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.set = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  o.subarray = function (...args: any[]): any { calls++; return "FAKE(" + args.length + ")"; };
  return o as BigInt64Array; }
function clsB64(): BigInt64Array { const o: any = new KB64(); return o as BigInt64Array; }
function nullB64(): BigInt64Array { const o: any = JSON.parse("null"); return o as BigInt64Array; }
function undefB64(): BigInt64Array { const o: any = JSON.parse("[]")[0]; return o as BigInt64Array; }
function PB64_at(a: BigInt64Array): any { return a.at(0); }
function SB64_at(a: BigInt64Array): void { a.at(0); }
class FB64_at { k: BigInt64Array = mkB64(); run(): any { return this.k.at(0); } }
function PB64_fill(a: BigInt64Array): any { return a.fill(5n); }
function SB64_fill(a: BigInt64Array): void { a.fill(5n); }
class FB64_fill { k: BigInt64Array = mkB64(); run(): any { return this.k.fill(5n); } }
function PB64_indexOf(a: BigInt64Array): any { return a.indexOf(1n); }
function SB64_indexOf(a: BigInt64Array): void { a.indexOf(1n); }
class FB64_indexOf { k: BigInt64Array = mkB64(); run(): any { return this.k.indexOf(1n); } }
function PB64_includes(a: BigInt64Array): any { return a.includes(1n); }
function SB64_includes(a: BigInt64Array): void { a.includes(1n); }
class FB64_includes { k: BigInt64Array = mkB64(); run(): any { return this.k.includes(1n); } }
function PB64_slice(a: BigInt64Array): any { return a.slice(0, 2); }
function SB64_slice(a: BigInt64Array): void { a.slice(0, 2); }
class FB64_slice { k: BigInt64Array = mkB64(); run(): any { return this.k.slice(0, 2); } }
function PB64_join(a: BigInt64Array): any { return a.join("-"); }
function SB64_join(a: BigInt64Array): void { a.join("-"); }
class FB64_join { k: BigInt64Array = mkB64(); run(): any { return this.k.join("-"); } }
function PB64_reverse(a: BigInt64Array): any { return a.reverse(); }
function SB64_reverse(a: BigInt64Array): void { a.reverse(); }
class FB64_reverse { k: BigInt64Array = mkB64(); run(): any { return this.k.reverse(); } }
function PB64_map(a: BigInt64Array): any { return a.map((x: any) => x); }
function SB64_map(a: BigInt64Array): void { a.map((x: any) => x); }
class FB64_map { k: BigInt64Array = mkB64(); run(): any { return this.k.map((x: any) => x); } }
function PB64_filter(a: BigInt64Array): any { return a.filter((x: any) => x > 1n); }
function SB64_filter(a: BigInt64Array): void { a.filter((x: any) => x > 1n); }
class FB64_filter { k: BigInt64Array = mkB64(); run(): any { return this.k.filter((x: any) => x > 1n); } }
function PB64_forEach(a: BigInt64Array): any { return a.forEach((x: any) => { sink += Number(x); }); }
function SB64_forEach(a: BigInt64Array): void { a.forEach((x: any) => { sink += Number(x); }); }
class FB64_forEach { k: BigInt64Array = mkB64(); run(): any { return this.k.forEach((x: any) => { sink += Number(x); }); } }
function PB64_some(a: BigInt64Array): any { return a.some((x: any) => x > 1n); }
function SB64_some(a: BigInt64Array): void { a.some((x: any) => x > 1n); }
class FB64_some { k: BigInt64Array = mkB64(); run(): any { return this.k.some((x: any) => x > 1n); } }
function PB64_find(a: BigInt64Array): any { return a.find((x: any) => x > 1n); }
function SB64_find(a: BigInt64Array): void { a.find((x: any) => x > 1n); }
class FB64_find { k: BigInt64Array = mkB64(); run(): any { return this.k.find((x: any) => x > 1n); } }
function PB64_reduce(a: BigInt64Array): any { return a.reduce((p: any, x: any) => p + x, 0n); }
function SB64_reduce(a: BigInt64Array): void { a.reduce((p: any, x: any) => p + x, 0n); }
class FB64_reduce { k: BigInt64Array = mkB64(); run(): any { return this.k.reduce((p: any, x: any) => p + x, 0n); } }
function PB64_sort(a: BigInt64Array): any { return a.sort(); }
function SB64_sort(a: BigInt64Array): void { a.sort(); }
class FB64_sort { k: BigInt64Array = mkB64(); run(): any { return this.k.sort(); } }
function PB64_keys(a: BigInt64Array): any { return a.keys(); }
function SB64_keys(a: BigInt64Array): void { a.keys(); }
class FB64_keys { k: BigInt64Array = mkB64(); run(): any { return this.k.keys(); } }
function PB64_values(a: BigInt64Array): any { return a.values(); }
function SB64_values(a: BigInt64Array): void { a.values(); }
class FB64_values { k: BigInt64Array = mkB64(); run(): any { return this.k.values(); } }
function PB64_with(a: BigInt64Array): any { return a.with(0, 9n); }
function SB64_with(a: BigInt64Array): void { a.with(0, 9n); }
class FB64_with { k: BigInt64Array = mkB64(); run(): any { return this.k.with(0, 9n); } }
function PB64_toSorted(a: BigInt64Array): any { return a.toSorted(); }
function SB64_toSorted(a: BigInt64Array): void { a.toSorted(); }
class FB64_toSorted { k: BigInt64Array = mkB64(); run(): any { return this.k.toSorted(); } }
function PB64_set(a: BigInt64Array): any { return a.set([1n]); }
function SB64_set(a: BigInt64Array): void { a.set([1n]); }
class FB64_set { k: BigInt64Array = mkB64(); run(): any { return this.k.set([1n]); } }
function PB64_subarray(a: BigInt64Array): any { return a.subarray(1); }
function SB64_subarray(a: BigInt64Array): void { a.subarray(1); }
class FB64_subarray { k: BigInt64Array = mkB64(); run(): any { return this.k.subarray(1); } }
let GB64: BigInt64Array = mkB64();
function GB64_at(): any { return GB64.at(0); }
function GB64_fill(): any { return GB64.fill(5n); }
function GB64_indexOf(): any { return GB64.indexOf(1n); }
function GB64_includes(): any { return GB64.includes(1n); }
function GB64_slice(): any { return GB64.slice(0, 2); }
function GB64_join(): any { return GB64.join("-"); }
function GB64_reverse(): any { return GB64.reverse(); }
function GB64_map(): any { return GB64.map((x: any) => x); }
function GB64_filter(): any { return GB64.filter((x: any) => x > 1n); }
function GB64_forEach(): any { return GB64.forEach((x: any) => { sink += Number(x); }); }
function GB64_some(): any { return GB64.some((x: any) => x > 1n); }
function GB64_find(): any { return GB64.find((x: any) => x > 1n); }
function GB64_reduce(): any { return GB64.reduce((p: any, x: any) => p + x, 0n); }
function GB64_sort(): any { return GB64.sort(); }
function GB64_keys(): any { return GB64.keys(); }
function GB64_values(): any { return GB64.values(); }
function GB64_with(): any { return GB64.with(0, 9n); }
function GB64_toSorted(): any { return GB64.toSorted(); }
function GB64_set(): any { return GB64.set([1n]); }
function GB64_subarray(): any { return GB64.subarray(1); }
{ const c: BigInt64Array = mkB64(); T("B64 P at real", () => PB64_at(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S at real", () => { SB64_at(c); return 0; }, c); }
{ const h = new FB64_at(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F at real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G at real", () => GB64_at(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L at real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P fill real", () => PB64_fill(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S fill real", () => { SB64_fill(c); return 0; }, c); }
{ const h = new FB64_fill(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F fill real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G fill real", () => GB64_fill(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L fill real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P indexOf real", () => PB64_indexOf(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S indexOf real", () => { SB64_indexOf(c); return 0; }, c); }
{ const h = new FB64_indexOf(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F indexOf real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G indexOf real", () => GB64_indexOf(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L indexOf real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P includes real", () => PB64_includes(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S includes real", () => { SB64_includes(c); return 0; }, c); }
{ const h = new FB64_includes(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F includes real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G includes real", () => GB64_includes(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L includes real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P slice real", () => PB64_slice(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S slice real", () => { SB64_slice(c); return 0; }, c); }
{ const h = new FB64_slice(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F slice real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G slice real", () => GB64_slice(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L slice real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P join real", () => PB64_join(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S join real", () => { SB64_join(c); return 0; }, c); }
{ const h = new FB64_join(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F join real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G join real", () => GB64_join(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L join real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P reverse real", () => PB64_reverse(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S reverse real", () => { SB64_reverse(c); return 0; }, c); }
{ const h = new FB64_reverse(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F reverse real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G reverse real", () => GB64_reverse(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reverse real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P map real", () => PB64_map(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S map real", () => { SB64_map(c); return 0; }, c); }
{ const h = new FB64_map(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F map real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G map real", () => GB64_map(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L map real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P filter real", () => PB64_filter(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S filter real", () => { SB64_filter(c); return 0; }, c); }
{ const h = new FB64_filter(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F filter real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G filter real", () => GB64_filter(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L filter real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P forEach real", () => PB64_forEach(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S forEach real", () => { SB64_forEach(c); return 0; }, c); }
{ const h = new FB64_forEach(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F forEach real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G forEach real", () => GB64_forEach(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L forEach real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P some real", () => PB64_some(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S some real", () => { SB64_some(c); return 0; }, c); }
{ const h = new FB64_some(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F some real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G some real", () => GB64_some(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L some real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P find real", () => PB64_find(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S find real", () => { SB64_find(c); return 0; }, c); }
{ const h = new FB64_find(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F find real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G find real", () => GB64_find(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L find real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P reduce real", () => PB64_reduce(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S reduce real", () => { SB64_reduce(c); return 0; }, c); }
{ const h = new FB64_reduce(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F reduce real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G reduce real", () => GB64_reduce(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reduce real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P sort real", () => PB64_sort(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S sort real", () => { SB64_sort(c); return 0; }, c); }
{ const h = new FB64_sort(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F sort real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G sort real", () => GB64_sort(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L sort real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P keys real", () => PB64_keys(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S keys real", () => { SB64_keys(c); return 0; }, c); }
{ const h = new FB64_keys(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F keys real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G keys real", () => GB64_keys(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L keys real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P values real", () => PB64_values(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S values real", () => { SB64_values(c); return 0; }, c); }
{ const h = new FB64_values(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F values real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G values real", () => GB64_values(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L values real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P with real", () => PB64_with(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S with real", () => { SB64_with(c); return 0; }, c); }
{ const h = new FB64_with(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F with real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G with real", () => GB64_with(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L with real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P toSorted real", () => PB64_toSorted(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S toSorted real", () => { SB64_toSorted(c); return 0; }, c); }
{ const h = new FB64_toSorted(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F toSorted real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G toSorted real", () => GB64_toSorted(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L toSorted real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P set real", () => PB64_set(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S set real", () => { SB64_set(c); return 0; }, c); }
{ const h = new FB64_set(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F set real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G set real", () => GB64_set(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1n]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L set real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = mkB64(); T("B64 P subarray real", () => PB64_subarray(c), c); }
{ const c: BigInt64Array = mkB64(); T("B64 S subarray real", () => { SB64_subarray(c); return 0; }, c); }
{ const h = new FB64_subarray(); const c: BigInt64Array = mkB64(); h.k = c; T("B64 F subarray real", () => h.run(), h.k); }
{ GB64 = mkB64(); T("B64 G subarray real", () => GB64_subarray(), GB64); }
{ const c: BigInt64Array = mkB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L subarray real -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P at fake", () => PB64_at(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S at fake", () => { SB64_at(c); return 0; }, c); }
{ const h = new FB64_at(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F at fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G at fake", () => GB64_at(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L at fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P fill fake", () => PB64_fill(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S fill fake", () => { SB64_fill(c); return 0; }, c); }
{ const h = new FB64_fill(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F fill fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G fill fake", () => GB64_fill(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L fill fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P indexOf fake", () => PB64_indexOf(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S indexOf fake", () => { SB64_indexOf(c); return 0; }, c); }
{ const h = new FB64_indexOf(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F indexOf fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G indexOf fake", () => GB64_indexOf(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L indexOf fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P includes fake", () => PB64_includes(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S includes fake", () => { SB64_includes(c); return 0; }, c); }
{ const h = new FB64_includes(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F includes fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G includes fake", () => GB64_includes(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L includes fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P slice fake", () => PB64_slice(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S slice fake", () => { SB64_slice(c); return 0; }, c); }
{ const h = new FB64_slice(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F slice fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G slice fake", () => GB64_slice(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L slice fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P join fake", () => PB64_join(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S join fake", () => { SB64_join(c); return 0; }, c); }
{ const h = new FB64_join(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F join fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G join fake", () => GB64_join(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L join fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P reverse fake", () => PB64_reverse(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S reverse fake", () => { SB64_reverse(c); return 0; }, c); }
{ const h = new FB64_reverse(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F reverse fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G reverse fake", () => GB64_reverse(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reverse fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P map fake", () => PB64_map(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S map fake", () => { SB64_map(c); return 0; }, c); }
{ const h = new FB64_map(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F map fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G map fake", () => GB64_map(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L map fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P filter fake", () => PB64_filter(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S filter fake", () => { SB64_filter(c); return 0; }, c); }
{ const h = new FB64_filter(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F filter fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G filter fake", () => GB64_filter(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L filter fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P forEach fake", () => PB64_forEach(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S forEach fake", () => { SB64_forEach(c); return 0; }, c); }
{ const h = new FB64_forEach(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F forEach fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G forEach fake", () => GB64_forEach(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L forEach fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P some fake", () => PB64_some(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S some fake", () => { SB64_some(c); return 0; }, c); }
{ const h = new FB64_some(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F some fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G some fake", () => GB64_some(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L some fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P find fake", () => PB64_find(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S find fake", () => { SB64_find(c); return 0; }, c); }
{ const h = new FB64_find(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F find fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G find fake", () => GB64_find(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L find fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P reduce fake", () => PB64_reduce(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S reduce fake", () => { SB64_reduce(c); return 0; }, c); }
{ const h = new FB64_reduce(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F reduce fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G reduce fake", () => GB64_reduce(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reduce fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P sort fake", () => PB64_sort(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S sort fake", () => { SB64_sort(c); return 0; }, c); }
{ const h = new FB64_sort(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F sort fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G sort fake", () => GB64_sort(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L sort fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P keys fake", () => PB64_keys(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S keys fake", () => { SB64_keys(c); return 0; }, c); }
{ const h = new FB64_keys(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F keys fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G keys fake", () => GB64_keys(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L keys fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P values fake", () => PB64_values(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S values fake", () => { SB64_values(c); return 0; }, c); }
{ const h = new FB64_values(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F values fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G values fake", () => GB64_values(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L values fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P with fake", () => PB64_with(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S with fake", () => { SB64_with(c); return 0; }, c); }
{ const h = new FB64_with(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F with fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G with fake", () => GB64_with(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L with fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P toSorted fake", () => PB64_toSorted(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S toSorted fake", () => { SB64_toSorted(c); return 0; }, c); }
{ const h = new FB64_toSorted(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F toSorted fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G toSorted fake", () => GB64_toSorted(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L toSorted fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P set fake", () => PB64_set(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S set fake", () => { SB64_set(c); return 0; }, c); }
{ const h = new FB64_set(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F set fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G set fake", () => GB64_set(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1n]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L set fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = fakeB64(); T("B64 P subarray fake", () => PB64_subarray(c), c); }
{ const c: BigInt64Array = fakeB64(); T("B64 S subarray fake", () => { SB64_subarray(c); return 0; }, c); }
{ const h = new FB64_subarray(); const c: BigInt64Array = fakeB64(); h.k = c; T("B64 F subarray fake", () => h.run(), h.k); }
{ GB64 = fakeB64(); T("B64 G subarray fake", () => GB64_subarray(), GB64); }
{ const c: BigInt64Array = fakeB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L subarray fake -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P at cls", () => PB64_at(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S at cls", () => { SB64_at(c); return 0; }, c); }
{ const h = new FB64_at(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F at cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G at cls", () => GB64_at(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L at cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P fill cls", () => PB64_fill(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S fill cls", () => { SB64_fill(c); return 0; }, c); }
{ const h = new FB64_fill(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F fill cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G fill cls", () => GB64_fill(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L fill cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P indexOf cls", () => PB64_indexOf(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S indexOf cls", () => { SB64_indexOf(c); return 0; }, c); }
{ const h = new FB64_indexOf(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F indexOf cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G indexOf cls", () => GB64_indexOf(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L indexOf cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P includes cls", () => PB64_includes(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S includes cls", () => { SB64_includes(c); return 0; }, c); }
{ const h = new FB64_includes(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F includes cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G includes cls", () => GB64_includes(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L includes cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P slice cls", () => PB64_slice(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S slice cls", () => { SB64_slice(c); return 0; }, c); }
{ const h = new FB64_slice(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F slice cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G slice cls", () => GB64_slice(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L slice cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P join cls", () => PB64_join(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S join cls", () => { SB64_join(c); return 0; }, c); }
{ const h = new FB64_join(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F join cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G join cls", () => GB64_join(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L join cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P reverse cls", () => PB64_reverse(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S reverse cls", () => { SB64_reverse(c); return 0; }, c); }
{ const h = new FB64_reverse(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F reverse cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G reverse cls", () => GB64_reverse(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reverse cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P map cls", () => PB64_map(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S map cls", () => { SB64_map(c); return 0; }, c); }
{ const h = new FB64_map(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F map cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G map cls", () => GB64_map(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L map cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P filter cls", () => PB64_filter(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S filter cls", () => { SB64_filter(c); return 0; }, c); }
{ const h = new FB64_filter(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F filter cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G filter cls", () => GB64_filter(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L filter cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P forEach cls", () => PB64_forEach(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S forEach cls", () => { SB64_forEach(c); return 0; }, c); }
{ const h = new FB64_forEach(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F forEach cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G forEach cls", () => GB64_forEach(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L forEach cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P some cls", () => PB64_some(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S some cls", () => { SB64_some(c); return 0; }, c); }
{ const h = new FB64_some(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F some cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G some cls", () => GB64_some(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L some cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P find cls", () => PB64_find(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S find cls", () => { SB64_find(c); return 0; }, c); }
{ const h = new FB64_find(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F find cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G find cls", () => GB64_find(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L find cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P reduce cls", () => PB64_reduce(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S reduce cls", () => { SB64_reduce(c); return 0; }, c); }
{ const h = new FB64_reduce(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F reduce cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G reduce cls", () => GB64_reduce(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reduce cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P sort cls", () => PB64_sort(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S sort cls", () => { SB64_sort(c); return 0; }, c); }
{ const h = new FB64_sort(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F sort cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G sort cls", () => GB64_sort(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L sort cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P keys cls", () => PB64_keys(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S keys cls", () => { SB64_keys(c); return 0; }, c); }
{ const h = new FB64_keys(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F keys cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G keys cls", () => GB64_keys(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L keys cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P values cls", () => PB64_values(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S values cls", () => { SB64_values(c); return 0; }, c); }
{ const h = new FB64_values(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F values cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G values cls", () => GB64_values(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L values cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P with cls", () => PB64_with(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S with cls", () => { SB64_with(c); return 0; }, c); }
{ const h = new FB64_with(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F with cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G with cls", () => GB64_with(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L with cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P toSorted cls", () => PB64_toSorted(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S toSorted cls", () => { SB64_toSorted(c); return 0; }, c); }
{ const h = new FB64_toSorted(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F toSorted cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G toSorted cls", () => GB64_toSorted(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L toSorted cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P set cls", () => PB64_set(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S set cls", () => { SB64_set(c); return 0; }, c); }
{ const h = new FB64_set(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F set cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G set cls", () => GB64_set(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1n]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L set cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = clsB64(); T("B64 P subarray cls", () => PB64_subarray(c), c); }
{ const c: BigInt64Array = clsB64(); T("B64 S subarray cls", () => { SB64_subarray(c); return 0; }, c); }
{ const h = new FB64_subarray(); const c: BigInt64Array = clsB64(); h.k = c; T("B64 F subarray cls", () => h.run(), h.k); }
{ GB64 = clsB64(); T("B64 G subarray cls", () => GB64_subarray(), GB64); }
{ const c: BigInt64Array = clsB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L subarray cls -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P at sub", () => PB64_at(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S at sub", () => { SB64_at(c); return 0; }, c); }
{ const h = new FB64_at(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F at sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L at sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P fill sub", () => PB64_fill(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S fill sub", () => { SB64_fill(c); return 0; }, c); }
{ const h = new FB64_fill(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F fill sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L fill sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P indexOf sub", () => PB64_indexOf(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S indexOf sub", () => { SB64_indexOf(c); return 0; }, c); }
{ const h = new FB64_indexOf(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F indexOf sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L indexOf sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P includes sub", () => PB64_includes(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S includes sub", () => { SB64_includes(c); return 0; }, c); }
{ const h = new FB64_includes(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F includes sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L includes sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P slice sub", () => PB64_slice(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S slice sub", () => { SB64_slice(c); return 0; }, c); }
{ const h = new FB64_slice(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F slice sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L slice sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P join sub", () => PB64_join(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S join sub", () => { SB64_join(c); return 0; }, c); }
{ const h = new FB64_join(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F join sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L join sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P reverse sub", () => PB64_reverse(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S reverse sub", () => { SB64_reverse(c); return 0; }, c); }
{ const h = new FB64_reverse(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F reverse sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reverse sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P map sub", () => PB64_map(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S map sub", () => { SB64_map(c); return 0; }, c); }
{ const h = new FB64_map(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F map sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L map sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P filter sub", () => PB64_filter(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S filter sub", () => { SB64_filter(c); return 0; }, c); }
{ const h = new FB64_filter(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F filter sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L filter sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P forEach sub", () => PB64_forEach(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S forEach sub", () => { SB64_forEach(c); return 0; }, c); }
{ const h = new FB64_forEach(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F forEach sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L forEach sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P some sub", () => PB64_some(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S some sub", () => { SB64_some(c); return 0; }, c); }
{ const h = new FB64_some(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F some sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L some sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P find sub", () => PB64_find(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S find sub", () => { SB64_find(c); return 0; }, c); }
{ const h = new FB64_find(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F find sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L find sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P reduce sub", () => PB64_reduce(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S reduce sub", () => { SB64_reduce(c); return 0; }, c); }
{ const h = new FB64_reduce(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F reduce sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reduce sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P sort sub", () => PB64_sort(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S sort sub", () => { SB64_sort(c); return 0; }, c); }
{ const h = new FB64_sort(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F sort sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L sort sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P keys sub", () => PB64_keys(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S keys sub", () => { SB64_keys(c); return 0; }, c); }
{ const h = new FB64_keys(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F keys sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L keys sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P values sub", () => PB64_values(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S values sub", () => { SB64_values(c); return 0; }, c); }
{ const h = new FB64_values(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F values sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L values sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P with sub", () => PB64_with(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S with sub", () => { SB64_with(c); return 0; }, c); }
{ const h = new FB64_with(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F with sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L with sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P toSorted sub", () => PB64_toSorted(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S toSorted sub", () => { SB64_toSorted(c); return 0; }, c); }
{ const h = new FB64_toSorted(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F toSorted sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L toSorted sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P set sub", () => PB64_set(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S set sub", () => { SB64_set(c); return 0; }, c); }
{ const h = new FB64_set(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F set sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1n]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L set sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = subB64(); T("B64 P subarray sub", () => PB64_subarray(c), c); }
{ const c: BigInt64Array = subB64(); T("B64 S subarray sub", () => { SB64_subarray(c); return 0; }, c); }
{ const h = new FB64_subarray(); const c: BigInt64Array = subB64(); h.k = c; T("B64 F subarray sub", () => h.run(), h.k); }
{ const c: BigInt64Array = subB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L subarray sub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P at psub", () => PB64_at(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S at psub", () => { SB64_at(c); return 0; }, c); }
{ const h = new FB64_at(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F at psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G at psub", () => GB64_at(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L at psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P fill psub", () => PB64_fill(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S fill psub", () => { SB64_fill(c); return 0; }, c); }
{ const h = new FB64_fill(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F fill psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G fill psub", () => GB64_fill(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L fill psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P indexOf psub", () => PB64_indexOf(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S indexOf psub", () => { SB64_indexOf(c); return 0; }, c); }
{ const h = new FB64_indexOf(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F indexOf psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G indexOf psub", () => GB64_indexOf(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L indexOf psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P includes psub", () => PB64_includes(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S includes psub", () => { SB64_includes(c); return 0; }, c); }
{ const h = new FB64_includes(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F includes psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G includes psub", () => GB64_includes(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L includes psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P slice psub", () => PB64_slice(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S slice psub", () => { SB64_slice(c); return 0; }, c); }
{ const h = new FB64_slice(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F slice psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G slice psub", () => GB64_slice(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L slice psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P join psub", () => PB64_join(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S join psub", () => { SB64_join(c); return 0; }, c); }
{ const h = new FB64_join(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F join psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G join psub", () => GB64_join(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L join psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P reverse psub", () => PB64_reverse(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S reverse psub", () => { SB64_reverse(c); return 0; }, c); }
{ const h = new FB64_reverse(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F reverse psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G reverse psub", () => GB64_reverse(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reverse psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P map psub", () => PB64_map(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S map psub", () => { SB64_map(c); return 0; }, c); }
{ const h = new FB64_map(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F map psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G map psub", () => GB64_map(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L map psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P filter psub", () => PB64_filter(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S filter psub", () => { SB64_filter(c); return 0; }, c); }
{ const h = new FB64_filter(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F filter psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G filter psub", () => GB64_filter(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L filter psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P forEach psub", () => PB64_forEach(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S forEach psub", () => { SB64_forEach(c); return 0; }, c); }
{ const h = new FB64_forEach(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F forEach psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G forEach psub", () => GB64_forEach(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L forEach psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P some psub", () => PB64_some(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S some psub", () => { SB64_some(c); return 0; }, c); }
{ const h = new FB64_some(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F some psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G some psub", () => GB64_some(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L some psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P find psub", () => PB64_find(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S find psub", () => { SB64_find(c); return 0; }, c); }
{ const h = new FB64_find(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F find psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G find psub", () => GB64_find(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L find psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P reduce psub", () => PB64_reduce(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S reduce psub", () => { SB64_reduce(c); return 0; }, c); }
{ const h = new FB64_reduce(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F reduce psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G reduce psub", () => GB64_reduce(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reduce psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P sort psub", () => PB64_sort(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S sort psub", () => { SB64_sort(c); return 0; }, c); }
{ const h = new FB64_sort(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F sort psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G sort psub", () => GB64_sort(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L sort psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P keys psub", () => PB64_keys(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S keys psub", () => { SB64_keys(c); return 0; }, c); }
{ const h = new FB64_keys(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F keys psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G keys psub", () => GB64_keys(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L keys psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P values psub", () => PB64_values(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S values psub", () => { SB64_values(c); return 0; }, c); }
{ const h = new FB64_values(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F values psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G values psub", () => GB64_values(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L values psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P with psub", () => PB64_with(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S with psub", () => { SB64_with(c); return 0; }, c); }
{ const h = new FB64_with(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F with psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G with psub", () => GB64_with(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L with psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P toSorted psub", () => PB64_toSorted(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S toSorted psub", () => { SB64_toSorted(c); return 0; }, c); }
{ const h = new FB64_toSorted(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F toSorted psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G toSorted psub", () => GB64_toSorted(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L toSorted psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P set psub", () => PB64_set(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S set psub", () => { SB64_set(c); return 0; }, c); }
{ const h = new FB64_set(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F set psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G set psub", () => GB64_set(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1n]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L set psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = psubB64(); T("B64 P subarray psub", () => PB64_subarray(c), c); }
{ const c: BigInt64Array = psubB64(); T("B64 S subarray psub", () => { SB64_subarray(c); return 0; }, c); }
{ const h = new FB64_subarray(); const c: BigInt64Array = psubB64(); h.k = c; T("B64 F subarray psub", () => h.run(), h.k); }
{ GB64 = psubB64(); T("B64 G subarray psub", () => GB64_subarray(), GB64); }
{ const c: BigInt64Array = psubB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L subarray psub -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P at null", () => PB64_at(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S at null", () => { SB64_at(c); return 0; }, c); }
{ const h = new FB64_at(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F at null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L at null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P fill null", () => PB64_fill(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S fill null", () => { SB64_fill(c); return 0; }, c); }
{ const h = new FB64_fill(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F fill null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L fill null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P indexOf null", () => PB64_indexOf(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S indexOf null", () => { SB64_indexOf(c); return 0; }, c); }
{ const h = new FB64_indexOf(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F indexOf null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L indexOf null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P includes null", () => PB64_includes(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S includes null", () => { SB64_includes(c); return 0; }, c); }
{ const h = new FB64_includes(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F includes null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L includes null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P slice null", () => PB64_slice(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S slice null", () => { SB64_slice(c); return 0; }, c); }
{ const h = new FB64_slice(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F slice null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L slice null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P join null", () => PB64_join(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S join null", () => { SB64_join(c); return 0; }, c); }
{ const h = new FB64_join(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F join null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L join null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P reverse null", () => PB64_reverse(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S reverse null", () => { SB64_reverse(c); return 0; }, c); }
{ const h = new FB64_reverse(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F reverse null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reverse null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P map null", () => PB64_map(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S map null", () => { SB64_map(c); return 0; }, c); }
{ const h = new FB64_map(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F map null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L map null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P filter null", () => PB64_filter(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S filter null", () => { SB64_filter(c); return 0; }, c); }
{ const h = new FB64_filter(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F filter null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L filter null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P forEach null", () => PB64_forEach(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S forEach null", () => { SB64_forEach(c); return 0; }, c); }
{ const h = new FB64_forEach(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F forEach null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L forEach null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P some null", () => PB64_some(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S some null", () => { SB64_some(c); return 0; }, c); }
{ const h = new FB64_some(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F some null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L some null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P find null", () => PB64_find(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S find null", () => { SB64_find(c); return 0; }, c); }
{ const h = new FB64_find(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F find null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L find null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P reduce null", () => PB64_reduce(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S reduce null", () => { SB64_reduce(c); return 0; }, c); }
{ const h = new FB64_reduce(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F reduce null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reduce null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P sort null", () => PB64_sort(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S sort null", () => { SB64_sort(c); return 0; }, c); }
{ const h = new FB64_sort(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F sort null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L sort null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P keys null", () => PB64_keys(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S keys null", () => { SB64_keys(c); return 0; }, c); }
{ const h = new FB64_keys(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F keys null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L keys null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P values null", () => PB64_values(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S values null", () => { SB64_values(c); return 0; }, c); }
{ const h = new FB64_values(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F values null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L values null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P with null", () => PB64_with(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S with null", () => { SB64_with(c); return 0; }, c); }
{ const h = new FB64_with(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F with null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L with null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P toSorted null", () => PB64_toSorted(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S toSorted null", () => { SB64_toSorted(c); return 0; }, c); }
{ const h = new FB64_toSorted(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F toSorted null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L toSorted null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P set null", () => PB64_set(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S set null", () => { SB64_set(c); return 0; }, c); }
{ const h = new FB64_set(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F set null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1n]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L set null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = nullB64(); T("B64 P subarray null", () => PB64_subarray(c), c); }
{ const c: BigInt64Array = nullB64(); T("B64 S subarray null", () => { SB64_subarray(c); return 0; }, c); }
{ const h = new FB64_subarray(); const c: BigInt64Array = nullB64(); h.k = c; T("B64 F subarray null", () => h.run(), h.k); }
{ const c: BigInt64Array = nullB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L subarray null -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P at undef", () => PB64_at(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S at undef", () => { SB64_at(c); return 0; }, c); }
{ const h = new FB64_at(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F at undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.at(0); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L at undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P fill undef", () => PB64_fill(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S fill undef", () => { SB64_fill(c); return 0; }, c); }
{ const h = new FB64_fill(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F fill undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.fill(5n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L fill undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P indexOf undef", () => PB64_indexOf(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S indexOf undef", () => { SB64_indexOf(c); return 0; }, c); }
{ const h = new FB64_indexOf(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F indexOf undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.indexOf(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L indexOf undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P includes undef", () => PB64_includes(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S includes undef", () => { SB64_includes(c); return 0; }, c); }
{ const h = new FB64_includes(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F includes undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.includes(1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L includes undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P slice undef", () => PB64_slice(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S slice undef", () => { SB64_slice(c); return 0; }, c); }
{ const h = new FB64_slice(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F slice undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.slice(0, 2); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L slice undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P join undef", () => PB64_join(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S join undef", () => { SB64_join(c); return 0; }, c); }
{ const h = new FB64_join(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F join undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.join("-"); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L join undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P reverse undef", () => PB64_reverse(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S reverse undef", () => { SB64_reverse(c); return 0; }, c); }
{ const h = new FB64_reverse(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F reverse undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reverse(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reverse undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P map undef", () => PB64_map(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S map undef", () => { SB64_map(c); return 0; }, c); }
{ const h = new FB64_map(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F map undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.map((x: any) => x); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L map undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P filter undef", () => PB64_filter(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S filter undef", () => { SB64_filter(c); return 0; }, c); }
{ const h = new FB64_filter(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F filter undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.filter((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L filter undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P forEach undef", () => PB64_forEach(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S forEach undef", () => { SB64_forEach(c); return 0; }, c); }
{ const h = new FB64_forEach(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F forEach undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.forEach((x: any) => { sink += Number(x); }); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L forEach undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P some undef", () => PB64_some(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S some undef", () => { SB64_some(c); return 0; }, c); }
{ const h = new FB64_some(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F some undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.some((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L some undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P find undef", () => PB64_find(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S find undef", () => { SB64_find(c); return 0; }, c); }
{ const h = new FB64_find(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F find undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.find((x: any) => x > 1n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L find undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P reduce undef", () => PB64_reduce(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S reduce undef", () => { SB64_reduce(c); return 0; }, c); }
{ const h = new FB64_reduce(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F reduce undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.reduce((p: any, x: any) => p + x, 0n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L reduce undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P sort undef", () => PB64_sort(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S sort undef", () => { SB64_sort(c); return 0; }, c); }
{ const h = new FB64_sort(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F sort undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.sort(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L sort undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P keys undef", () => PB64_keys(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S keys undef", () => { SB64_keys(c); return 0; }, c); }
{ const h = new FB64_keys(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F keys undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.keys(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L keys undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P values undef", () => PB64_values(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S values undef", () => { SB64_values(c); return 0; }, c); }
{ const h = new FB64_values(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F values undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.values(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L values undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P with undef", () => PB64_with(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S with undef", () => { SB64_with(c); return 0; }, c); }
{ const h = new FB64_with(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F with undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.with(0, 9n); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L with undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P toSorted undef", () => PB64_toSorted(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S toSorted undef", () => { SB64_toSorted(c); return 0; }, c); }
{ const h = new FB64_toSorted(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F toSorted undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.toSorted(); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L toSorted undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P set undef", () => PB64_set(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S set undef", () => { SB64_set(c); return 0; }, c); }
{ const h = new FB64_set(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F set undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.set([1n]); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L set undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
{ const c: BigInt64Array = undefB64(); T("B64 P subarray undef", () => PB64_subarray(c), c); }
{ const c: BigInt64Array = undefB64(); T("B64 S subarray undef", () => { SB64_subarray(c); return 0; }, c); }
{ const h = new FB64_subarray(); const c: BigInt64Array = undefB64(); h.k = c; T("B64 F subarray undef", () => h.run(), h.k); }
{ const c: BigInt64Array = undefB64(); const c0 = calls; let r: any; let t = "ok"; try { r = c.subarray(1); } catch (e: any) { t = "THROW " + (e && e.name); } console.log("B64 L subarray undef -> " + show(r) + " " + t + " | " + st(c) + " c=" + (calls - c0)); }
function q5(a: Float64Array): any { return [a.fill(2 as any), a.at(0)]; }
console.log(JSON.stringify(q5({ fill() { return "F"; }, at() { return "A"; } } as any)));
function q4(a: Int16Array): any { a.set([1]); return a.length; }
console.log(q4({ set() { console.log("S"); }, length: 7 } as any));
function only(a: Float64Array): number { a.fill(1.5); a.reverse(); return a.at(-1)! + a.indexOf(1.5) + a.subarray(1).length; }
{ const a = new Float64Array(1000); let s = 0; for (let i = 0; i < 50; i++) s += only(a); console.log("control", s, a.length, calls); }
class Ring { buf: Float32Array = new Float32Array(8); n = 0; put(v: number): void { this.buf.fill(v, this.n, this.n + 1); this.n = (this.n + 1) & 7; } sum(): number { return this.buf.reduce((p, x) => p + x, 0); } }
{ const r = new Ring(); for (let i = 0; i < 20; i++) r.put(i); console.log("ring", r.sum(), r.buf.join(",")); }
