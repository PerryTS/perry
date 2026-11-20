// Object.create(null) objects are born ordinary with no [[Prototype]]: keyed
// and named adds skip the chain walk. Every spec observable of a
// prototype-less object must hold, before and after it takes keys, and after
// a later relink.

const keys = ["alpha", "beta", "__proto__", "toString", "constructor", "0", "hasOwnProperty"];

function show(label: string, v: unknown): void {
  console.log(label, JSON.stringify(v));
}

// 1. Adds through dynamic keys, including names Object.prototype holds.
const d: any = Object.create(null);
for (let i = 0; i < keys.length; i++) d[keys[i]] = i;
show("keys", Object.keys(d));
show("json", JSON.stringify(d));
show("proto", Object.getPrototypeOf(d));
show("own __proto__", Object.getOwnPropertyDescriptor(d, "__proto__"));
show("in", ["alpha" in d, "__proto__" in d, "valueOf" in d, "toString" in d]);
show("toString value", d.toString);
console.log("tag", Object.prototype.toString.call(d));
show("entries", Object.entries(d));

// 2. Named adds and the __proto__ literal key on a fresh dictionary.
const n: any = Object.create(null);
n.__proto__ = { evil: 1 };
n.a = 1;
show("named __proto__ own", Object.keys(n));
show("named evil", n.evil);
show("named proto", Object.getPrototypeOf(n));
const n2: any = Object.create(null);
n2["__proto__"] = 5;
show("keyed __proto__ number", [n2.__proto__, Object.keys(n2)]);

// 3. A prototype-less map (the dictionary idiom).
const counts: any = Object.create(null);
const words = "the __proto__ constructor the toString x the".split(" ");
for (const w of words) counts[w] = (counts[w] || 0) + 1;
show("counts", counts);
let forIn: string[] = [];
for (const k in counts) forIn.push(k);
show("for-in", forIn);

// 4. Missing keys read undefined; String() throws (no toString on the chain).
const m: any = Object.create(null);
m.x = 1;
show("missing", [m.y, m.toString, m.valueOf]);
try {
  console.log(String(m));
} catch (e) {
  console.log("String throws", (e as Error).constructor.name);
}
try {
  console.log(`${m}`);
} catch (e) {
  console.log("template throws", (e as Error).constructor.name);
}

// 5. setPrototypeOf later: to an object with a setter and a readonly, then back to null.
let setterHits = 0;
const P: any = {};
Object.defineProperty(P, "guarded", { set(v) { setterHits += v; }, get() { return "g"; } });
Object.defineProperty(P, "fixed", { value: 7, writable: false });
const s: any = Object.create(null);
s.before = 1;
Object.setPrototypeOf(s, P);
s.guarded = 10;
try {
  s.fixed = 99;
} catch (e) {
  console.log("readonly throws", (e as Error).constructor.name);
}
const sk = "guarded";
s[sk] = 5;
show("after relink", [setterHits, s.guarded, s.fixed, Object.keys(s)]);
Object.setPrototypeOf(s, null);
s.guarded = 3;
s.fixed = 4;
show("relinked null", [setterHits, s.guarded, s.fixed, Object.keys(s), Object.getPrototypeOf(s)]);

// 6. setPrototypeOf to Object.prototype: __proto__ becomes the accessor again.
const t: any = Object.create(null);
t.a = 1;
Object.setPrototypeOf(t, Object.prototype);
t.__proto__ = { inherited: 2 };
show("to Object.prototype", [Object.keys(t), t.inherited, String(t)]);
console.log("tag2", Object.prototype.toString.call(t));

// 7. Freeze, seal, preventExtensions.
const f: any = Object.create(null);
f.a = 1;
Object.freeze(f);
try {
  f.a = 2;
} catch (e) {
  console.log("frozen write throws", (e as Error).constructor.name);
}
try {
  f["b"] = 2;
} catch (e) {
  console.log("frozen add throws", (e as Error).constructor.name);
}
show("frozen", [f.a, f.b, Object.isFrozen(f), Object.keys(f)]);
const g: any = Object.create(null);
g.a = 1;
Object.seal(g);
g.a = 2;
try {
  g["b"] = 3;
} catch (e) {
  console.log("sealed add throws", (e as Error).constructor.name);
}
try {
  delete g.a;
} catch (e) {
  console.log("sealed delete throws", (e as Error).constructor.name);
}
show("sealed", [g.a, g.b, Object.isSealed(g), Object.keys(g)]);
const h: any = Object.create(null);
Object.preventExtensions(h);
try {
  h["z"] = 1;
} catch (e) {
  console.log("non-extensible add throws", (e as Error).constructor.name);
}
show("non-extensible", [h.z, Object.isExtensible(h)]);

// 8. Own accessors and readonly defined on the dictionary itself.
const o: any = Object.create(null);
let ownHits = 0;
Object.defineProperty(o, "acc", { set(v) { ownHits += v; }, configurable: true });
Object.defineProperty(o, "ro", { value: 1, writable: false, configurable: true });
const ak = "acc";
o[ak] = 4;
try {
  o["ro"] = 2;
} catch (e) {
  console.log("own readonly throws", (e as Error).constructor.name);
}
show("own descriptors", [ownHits, o.ro, Object.keys(o)]);

// 9. Many births at one site share a shape; a mixed site with literals stays distinct.
let total = 0;
for (let i = 0; i < 2000; i++) {
  const x: any = i & 1 ? Object.create(null) : {};
  x[keys[i % 2]] = i;
  total += Object.getPrototypeOf(x) === null ? 1 : 0;
  if (i === 1998) show("lit toString", typeof x.toString);
  if (i === 1999) show("null toString", typeof x.toString);
}
console.log("null births", total);

// 10. Object.assign / spread from and into a dictionary, delete, hasOwn.
const src: any = Object.create(null);
src.p = 1;
src["__proto__"] = 2;
const spread = { ...src };
show("spread", [Object.keys(spread), Object.getPrototypeOf(spread) === Object.prototype]);
const into: any = Object.assign(Object.create(null), { q: 1 }, src);
show("assign", [Object.keys(into), Object.getPrototypeOf(into)]);
delete into.q;
into.q = 3;
show("readd", [Object.keys(into), Object.hasOwn(into, "q"), "q" in into]);
