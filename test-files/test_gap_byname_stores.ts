// Stores by name: computed-key key-adds and overwrites answered from the
// receiver shape must keep [[Set]] semantics exactly (own data, inherited
// setter, frozen, non-writable, proxy, numeric-string keys), and class fields
// must keep DefineField semantics (an inherited setter never runs).
const out: string[] = [];
const KEYS = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"];
function put(o: any, k: any, v: any) { o[k] = v; }

// own data: add then overwrite, repeated shapes share the key-add edges
let sum = 0;
for (let i = 0; i < 3000; i++) {
  const o: any = {};
  for (let j = 0; j < KEYS.length; j++) put(o, KEYS[j], i + j);
  for (let j = 0; j < KEYS.length; j++) put(o, KEYS[j], (o as any)[KEYS[j]] + 1);
  sum += o.alpha + o.zeta;
}
out.push("own " + sum);

// fresh heap-string keys (not literals) on fresh objects
const parts = "a1=x&b22=y&c333=z&d4444=w".split("&");
let joined = "";
for (let i = 0; i < 2000; i++) {
  const o: any = {};
  for (const p of parts) { const eq = p.indexOf("="); put(o, p.slice(0, eq), p.slice(eq + 1) + i); }
  if (i % 999 === 0) joined += Object.keys(o).join(",") + ":" + o.c333 + ";";
}
out.push("heap " + joined);

// an inherited setter runs for a key the receiver lacks
const log: string[] = [];
const proto = { set watched(v: any) { log.push("set " + v); }, get watched() { return "g"; } };
for (let i = 0; i < 3; i++) { const o: any = Object.create(proto); put(o, "watched", i); put(o, "other", i); out.push(Object.keys(o).join(",") + " " + o.watched); }
out.push(log.join("|"));

// a setter installed on the prototype AFTER the shape learned its key-add
// edge: the edge exists, the prototype now intercepts, the setter must run
const late: any = {};
const lateA: any = Object.create(late);
put(lateA, "late", 1);
Object.defineProperty(late, "late", { set(v) { log.push("late setter " + v); }, configurable: true });
const lateB: any = Object.create(late);
put(lateB, "late", 2);
out.push("late " + Object.keys(lateA).join(",") + "|" + Object.keys(lateB).join(",") + "|" + log.join(","));

// Object.prototype setter and read-only inherited key
Object.defineProperty(Object.prototype, "opSetter", { set(v) { log.push("op " + v); }, configurable: true });
Object.defineProperty(Object.prototype, "opRo", { value: 1, writable: false, configurable: true });
for (let i = 0; i < 2; i++) {
  const o: any = {};
  put(o, "opSetter", i);
  try { put(o, "opRo", 9); } catch (e) { log.push("ro " + (e as Error).constructor.name); }
  out.push(JSON.stringify(Object.getOwnPropertyNames(o)) + " " + o.opRo);
}
delete (Object.prototype as any).opSetter;
delete (Object.prototype as any).opRo;
out.push(log.join("|"));

// frozen, sealed, non-extensible and non-writable receivers
const fr: any = Object.freeze({ a: 1 });
const se: any = Object.seal({ a: 1 });
const ne: any = Object.preventExtensions({ a: 1 });
const nw: any = {}; Object.defineProperty(nw, "a", { value: 1, writable: false, enumerable: true });
for (const [name, o] of [["frozen", fr], ["sealed", se], ["noext", ne], ["nonwritable", nw]] as const) {
  try { put(o, "a", 2); put(o, "b", 3); out.push(name + " " + JSON.stringify(o)); }
  catch (e) { out.push(name + " threw " + (e as Error).constructor.name + " " + JSON.stringify(o)); }
}

// a proxy receiver sees every store through its trap
const seen: string[] = [];
const px: any = new Proxy({}, { set(t: any, k, v) { seen.push(String(k)); t[k] = v * 2; return true; } });
for (const k of ["p", "q", "p"]) put(px, k, 5);
out.push("proxy " + seen.join(",") + " " + px.p + "," + px.q);

// numeric-string keys keep integer-key order
const num: any = {};
for (const k of ["b", "2", "a", "1", "10", "01"]) put(num, k, k);
out.push("numeric " + Object.keys(num).join(","));

// class fields are definitions: a base setter does not run, own value wins
const flog: string[] = [];
class Base { set f(v: any) { flog.push("base setter " + v); } get f() { return "base"; } }
class Derived extends Base { f = 1; g = 2; }
for (let i = 0; i < 3; i++) { const d: any = new Derived(); out.push("field " + d.f + " " + d.g + " " + Object.keys(d).join(",")); }
out.push("setter ran " + flog.length);

// stores interleaved with collections
let acc = 0;
for (let i = 0; i < 20000; i++) {
  const o: any = {};
  put(o, "k" + (i & 7), [i]);
  put(o, "v", { n: i });
  acc += o["k" + (i & 7)][0] + o.v.n;
}
out.push("gc " + acc);
console.log(out.join("\n"));
