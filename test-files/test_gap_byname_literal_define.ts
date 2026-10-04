// Object literals that take the source-ordered path (spread, accessor,
// computed key): every property is a DEFINITION (CreateDataPropertyOrThrow),
// answered from the receiver shape when it can be; the spread copies its
// source by position when the source shape proves every key a plain data
// property. Prototype setters and read-only inherited keys never interfere.
const log: string[] = [];
Object.defineProperty(Object.prototype, "trap", {
  set(v) { log.push("setter ran " + v); },
  get() { return "inherited"; },
  configurable: true,
});
const src: any = { a: 1 };
const o: any = { ...src, trap: 5, b: 2 };
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(o, "trap")), o.trap, log.length, Object.keys(o).join(","));
delete (Object.prototype as any).trap;

// readonly inherited key: define still creates own
Object.defineProperty(Object.prototype, "ro", { value: 1, writable: false, configurable: true });
const o2: any = { ...src, ro: 7 };
console.log(o2.ro, Object.keys(o2).join(","));
delete (Object.prototype as any).ro;

// later spread overrides earlier static, static overrides earlier spread
const o3: any = { x: 1, ...{ x: 2, y: 3 }, y: 4 };
console.log(JSON.stringify(o3));

// accessor in the same literal replaced by a later data key
const o4: any = { get z() { return 1; }, ...src, z: 9 };
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(o4, "z")));

// many shapes, numeric-like and long keys, __proto__ literal
const base = { inherited: 1 };
for (let i = 0; i < 3; i++) {
  const p: any = { __proto__: base, ...src, k0: i, "10": "ten", long_key_name_here: i * 2 };
  console.log(Object.keys(p).join(","), p.inherited, p.k0, p[10], p.long_key_name_here);
}
// frozen spread source and repeated shapes (edge reuse)
let acc = 0;
for (let i = 0; i < 2000; i++) { const q: any = { ...src, s1: i, s2: i + 1, s3: "v" }; acc += q.s1 + q.s2 + q.a; }
console.log(acc);
// spread keys are defined too: an Object.prototype setter for a spread key never runs
const log2: string[] = [];
Object.defineProperty(Object.prototype, "sp", { set(v) { log2.push("ran " + v); }, get() { return "inh"; }, configurable: true });
Object.defineProperty(Object.prototype, "ro2", { value: 0, writable: false, configurable: true });
const s2: any = { sp: 1, ro2: 2, plain: 3 };
const sym = Symbol("k");
s2[sym] = 4;
const o5: any = { pre: 0, ...s2 };
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(o5, "sp")), o5.ro2, o5[sym], log2.length, Object.keys(o5).join(","));
delete (Object.prototype as any).sp; delete (Object.prototype as any).ro2;
// a literal with an explicit prototype: its spread keys are still defined,
// never assigned through the prototype's setter (also after the shape has
// learned the key-add edge from an earlier literal)
const log3: string[] = [];
const sproto: any = {};
const first: any = { __proto__: sproto, ...{ k: 0 } };
Object.defineProperty(sproto, "k", { set(v) { log3.push("k " + v); }, configurable: true });
const second: any = { __proto__: sproto, ...{ k: 1 } };
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(first, "k")), JSON.stringify(Object.getOwnPropertyDescriptor(second, "k")), log3.length);
// getter on the source runs once, in order
let calls = 0;
const g: any = { get v() { calls++; return calls; }, w: 1 };
const o6: any = { ...g, ...g };
console.log(o6.v, calls, Object.keys(o6).join(","));

// ---- spread sources ----
const out2: string[] = [];
function show(label: string, o: any) { out2.push(label + " " + JSON.stringify(o) + " " + Object.keys(o).join(",")); }
// plain, with deletes, with many keys (spill), with SSO and long keys
const a: any = { x: 1, y: "two", z: null, u: undefined };
show("plain", { ...a });
const d: any = { p: 1, q: 2, r: 3 }; delete d.q;
show("deleted", { ...d, after: 1 });
const big: any = {}; for (let i = 0; i < 20; i++) big["key_number_" + i] = i;
show("big", { ...big });
// non-enumerable and accessor sources take the general path
const ne: any = { vis: 1 }; Object.defineProperty(ne, "hid", { value: 2, enumerable: false });
show("nonenum", { ...ne });
let n = 0; const accSrc: any = { get g() { return ++n; }, h: 2 };
show("accessor", { ...accSrc }); out2.push("n=" + n);
// class instance and function-constructor prototype as sources
class K { f = 1; g = 2; m() { return 3; } }
show("class", { ...new K() });
function F(this: any) { this.own = 1; } (F as any).prototype.shared = 5;
show("fnproto", { ...(F as any).prototype });
// Object.assign onto existing keys, onto a frozen object, numeric keys order
const t: any = { x: 0, k: 1 };
Object.assign(t, a);
show("assign", t);
const fz = Object.freeze({ x: 1 });
try { Object.assign(fz, { x: 2 }); out2.push("no throw"); } catch (e) { out2.push("frozen threw " + (e as Error).constructor.name); }
show("numeric", { ...{ b: 1, 2: "two", 1: "one", a: 0 } });
// arrays and strings as spread sources
show("array", { ...[10, 20] });
show("string", { ..."hi" });
// symbols copy
const s = Symbol("s"); const ws: any = { w: 1 }; ws[s] = 7;
const cp: any = { ...ws }; out2.push("sym " + cp[s]);
// GC pressure while spreading
let total = 0;
for (let i = 0; i < 20000; i++) { const src: any = { aa: i, bb: [i], cc: { v: i }, dd: "s" + i }; const c: any = { ...src, ee: i }; total += c.aa + c.bb[0] + c.cc.v + c.ee + c.dd.length; }
out2.push("total " + total);
console.log(out2.join("\n"));
