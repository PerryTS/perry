// Index keys on plain objects: the elements store a plain object takes for
// small index keys must be indistinguishable from ordinary own properties.
const show = (label: string, v: unknown) => console.log(label, JSON.stringify(v));

// Reads, writes, holes and the prototype chain.
const o: any = {};
o[3] = "c"; o[1] = "a"; o.name = "n"; o[2] = "b"; o["0"] = "z"; o[10] = "j";
show("keys", Object.keys(o));
show("values", Object.values(o));
show("entries", Object.entries(o));
show("json", o);
const forIn: string[] = [];
for (const k in o) forIn.push(k);
show("forin", forIn);
show("names", Object.getOwnPropertyNames(o));
show("ownKeys", Reflect.ownKeys(o));
console.log("reads", o[0], o[1], o["2"], o[3], o[4], o[10], o[11], o.length, o.name);
console.log("in", 1 in o, "1" in o, 4 in o, "length" in o, o.hasOwnProperty(2), o.hasOwnProperty("5"));
console.log("hasOwn", Object.hasOwn(o, 10), Object.hasOwn(o, 9), o.propertyIsEnumerable(3));
show("desc", Object.getOwnPropertyDescriptor(o, 3));
show("desc missing", Object.getOwnPropertyDescriptor(o, 4));
show("desc length", Object.getOwnPropertyDescriptor(o, "length"));
show("descs", Object.getOwnPropertyDescriptors(o));
console.log("delete", delete o[1], o[1], 1 in o, delete o[99], delete o["3"]);
show("after delete", Object.keys(o));
o[1] = "again";
show("re-add", Object.keys(o));
o.length = 5;
show("length is ordinary", [o.length, Object.keys(o)]);

// Overwrites of every kind of value, and growth with gaps.
const g: any = {};
for (let i = 0; i < 50; i++) g[i * 3] = i % 2 ? { i } : "s" + i;
g[0] = 1.5; g[3] = null; g[6] = undefined; g[9] = true;
show("grow", [Object.keys(g).length, g[0], g[3], g[6], g[9], g[12], g[13], g[147], g[148]]);
g[2000] = "far";
show("far", [Object.keys(g).length, Object.keys(g).slice(-3), g[2000], g[147]]);
g[1] = "after far";
show("after far", Object.keys(g).slice(0, 4));

// Inherited index keys, shadowing, and becoming a prototype.
const p: any = {};
p[4] = "proto4"; p[5] = "proto5";
const c: any = Object.create(p);
c[5] = "own5";
console.log("chain", c[4], c[5], c[6], 4 in c, c.hasOwnProperty(4));
show("chain keys", Object.keys(c));
const forInChain: string[] = [];
for (const k in c) forInChain.push(k);
show("chain forin", forInChain);
p[6] = "late";
console.log("late proto add", c[6]);

// A setter for an index key on the prototype intercepts the store.
const sp: any = {};
const log: unknown[] = [];
Object.defineProperty(sp, 7, { set(v) { log.push(v); }, get() { return "g7"; } });
const sc: any = Object.create(sp);
sc[7] = "x"; sc[8] = "y";
show("setter", [log, sc[7], sc[8], Object.keys(sc)]);
const setterProto: any = {};
Object.defineProperty(setterProto, 900, { set(v) { log.push("p" + v); } });
const viaProto: any = Object.setPrototypeOf({}, setterProto);
viaProto[900] = 1; viaProto[901] = 2;
show("custom proto setter", [log, Object.keys(viaProto), viaProto[901]]);

// Definitions, integrity levels.
const d: any = {};
d[0] = 0; d[1] = 1; d[2] = 2;
Object.defineProperty(d, 1, { value: "ro", writable: false, enumerable: false });
d[1] = "ignored";
show("defined", [d[1], Object.keys(d), Object.getOwnPropertyDescriptor(d, 1)]);
d[3] = 3;
show("after define", Object.keys(d));
const f: any = {};
f[0] = "a"; f[1] = "b";
Object.freeze(f);
f[0] = "changed"; f[2] = "new";
show("frozen", [f, Object.isFrozen(f), delete f[0]]);
const s: any = {};
s[0] = "a";
Object.seal(s);
s[0] = "ok"; s[1] = "no";
show("sealed", [s, Object.isSealed(s), delete s[0]]);
const n: any = {};
n[0] = "a";
Object.preventExtensions(n);
n[1] = "no"; n[0] = "yes";
show("non-extensible", [n, Object.isExtensible(n)]);
const e: any = {};
e[2] = 2;
console.log("extensible", Object.isExtensible(e), Object.isFrozen(e), Object.isSealed(e));

// Copies: spread, assign, structuredClone, JSON round trip.
const src: any = { b: 1 };
src[2] = "two"; src[0] = "zero";
show("spread", { ...src });
show("assign", Object.assign({}, src));
const tgt: any = {};
tgt[5] = "five";
Object.assign(tgt, src);
show("assign into", tgt);
show("clone", structuredClone(src));
show("parse", JSON.parse(JSON.stringify(src)));
show("fromEntries", Object.fromEntries(Object.entries(src)));
show("stringify pretty", JSON.stringify(src, null, 1));
show("stringify replacer", JSON.stringify(src, (k, v) => (k === "2" ? undefined : v)));
show("stringify keys", JSON.stringify(src, ["0", "b"]));

// Array-likes and generic Array methods.
const al: any = { length: 3 };
al[0] = "x"; al[1] = "y"; al[2] = "z";
show("arraylike", [Array.from(al), Array.prototype.join.call(al, "-"), Array.prototype.slice.call(al, 1)]);
Array.prototype.push.call(al, "w");
show("pushed", [al.length, al[3], Object.keys(al)]);

// Number spellings of the same key.
const k: any = {};
k[-0] = "zero"; k[1.0] = "one"; k["2"] = "two";
console.log("spellings", k[0], k["0"], k[1], k["1"], k[2], k["02"], k[" 2"], k[2.5]);
show("spellings keys", Object.keys(k));

// Maps keyed by id, as three.js keeps them.
const programs: any = {};
for (let id = 1; id <= 40; id++) programs[id] = { id };
let sum = 0;
for (let r = 0; r < 10; r++) for (let id = 1; id <= 40; id++) sum += programs[id].id;
delete programs[20];
console.log("programs", sum, programs[20], Object.keys(programs).length, programs[40].id);
const caps: any = {};
for (const id of [2929, 2884, 3042]) caps[id] = true;
caps[1] = "small";
show("mixed", [Object.keys(caps), caps[2929], caps[1]]);

// The same questions of objects built by computed index stores in a function.
function rec(keys: number[], extra: boolean): any {
  const o: any = {};
  for (let i = 0; i < keys.length; i++) o[keys[i] + 0] = "v" + keys[i];
  if (extra) o.tail = "t";
  return o;
}
const r = rec([4, 2, 9, 0], true);
console.log(r);
show("rec keys", [Object.keys(r), Object.getOwnPropertyNames(r), JSON.stringify(r), JSON.stringify(r, null, 1)]);
console.log("rec enumerable", r.propertyIsEnumerable(2), r.propertyIsEnumerable("9"), r.propertyIsEnumerable(3));
const target: any = {};
Object.defineProperties(target, rec([1, 0], false).constructor === Object ? {
  ...Object.fromEntries([["1", { value: "d1", enumerable: true }], ["0", { value: "d0", enumerable: true }]]),
} : {});
show("defineProperties", [Object.keys(target), target[0], target[1]]);
const bagged: any = {};
Object.defineProperties(bagged, (() => {
  const b: any = {};
  for (const k of [3, 1]) b[k + 0] = { value: "b" + k, enumerable: true, writable: true, configurable: true };
  return b;
})());
show("store bag", [Object.keys(bagged), bagged[1], bagged[3]]);
const del = rec([0, 1, 2], false);
console.log("delete", delete del["1"], 1 in del, Object.keys(del).join(","), delete del[5]);
console.log(rec([7, 3], false), rec([], true));
