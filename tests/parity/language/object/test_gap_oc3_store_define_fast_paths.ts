// OC3-S3: key adds on receivers that carry an own descriptor, fresh-key
// Object.defineProperty, and unchanged redefinitions (zod v4's
// `$constructor` pattern) must keep [[Set]]/[[DefineOwnProperty]] semantics.

function desc(o: any, k: string) {
  const d = Object.getOwnPropertyDescriptor(o, k);
  if (!d) return "absent";
  const parts: string[] = [];
  if ("value" in d) parts.push("value=" + String(d.value));
  if (d.get) parts.push("get");
  if (d.set) parts.push("set");
  parts.push("w=" + d.writable, "e=" + d.enumerable, "c=" + d.configurable);
  return parts.join(" ");
}

// 1. A receiver with a non-enumerable own key takes later keys by
//    assignment; an inherited setter or non-writable still intercepts.
let setterHits = 0;
const proto: any = {};
Object.defineProperty(proto, "viaSetter", { set(v: any) { setterHits += v; }, configurable: true });
Object.defineProperty(proto, "locked", { value: "proto", writable: false });
function Ctor(this: any) {
  Object.defineProperty(this, "_zod", { value: { n: 1 }, enumerable: false });
}
Ctor.prototype = proto;
const made: any[] = [];
for (let i = 0; i < 20; i++) {
  const o: any = new (Ctor as any)();
  o.a = i;
  o.b = i * 2;
  o.viaSetter = 1;
  try { o.locked = "own"; } catch (e) { if (i === 0) console.log("strict locked:", (e as Error).constructor.name); }
  o.c = "c" + i;
  made.push(o);
}
console.log("keys", JSON.stringify(Object.keys(made[3])), "setterHits", setterHits);
console.log("own viaSetter", Object.prototype.hasOwnProperty.call(made[3], "viaSetter"), "locked", made[3].locked);
console.log("_zod", desc(made[3], "_zod"), "a", desc(made[3], "a"));
for (const k in made[5]) console.log("for-in", k);

// 2. Fresh-key definitions: every attribute combination, data and accessor.
const fresh: any = { first: 1 };
Object.defineProperty(fresh, "d0", { value: 0 });
Object.defineProperty(fresh, "d1", { value: 1, writable: true });
Object.defineProperty(fresh, "d2", { value: 2, enumerable: true });
Object.defineProperty(fresh, "d3", { value: 3, configurable: true });
Object.defineProperty(fresh, "d4", { value: 4, writable: true, enumerable: true, configurable: false });
Object.defineProperty(fresh, "g0", { enumerable: true });
let backing = 10;
Object.defineProperty(fresh, "acc", {
  get() { return backing; },
  set(v: any) { backing = v * 2; },
  enumerable: true,
  configurable: true,
});
Object.defineProperty(fresh, "ro", { get() { return "ro:" + this.first; } });
for (const k of ["d0", "d1", "d2", "d3", "d4", "g0", "acc", "ro"]) console.log(k, desc(fresh, k));
fresh.acc = 7;
console.log("acc after set", fresh.acc, backing, "ro", fresh.ro);
console.log("keys", JSON.stringify(Object.keys(fresh)), "names", JSON.stringify(Object.getOwnPropertyNames(fresh)));
try { fresh.d0 = 99; } catch (e) { console.log("write d0:", (e as Error).constructor.name); }
try { fresh.ro = 1; } catch (e) { console.log("write ro:", (e as Error).constructor.name); }
console.log("d0", fresh.d0, "delete d3", delete fresh.d3, "d3", fresh.d3);
try { delete fresh.d0; } catch (e) { console.log("delete d0:", (e as Error).constructor.name); }

// 3. Unchanged redefinitions succeed; changing ones are validated.
const z: any = {};
const inner = { traits: 1 };
for (let i = 0; i < 4; i++) {
  Object.defineProperty(z, "_zod", { value: z._zod ?? inner, enumerable: false });
}
console.log("same object", z._zod === inner, desc(z, "_zod"));
try { Object.defineProperty(z, "_zod", { value: {} }); console.log("no throw"); } catch (e) { console.log("changed value:", (e as Error).constructor.name); }
try { Object.defineProperty(z, "_zod", { enumerable: true }); console.log("no throw"); } catch (e) { console.log("changed enumerable:", (e as Error).constructor.name); }
console.log("Reflect same", Reflect.defineProperty(z, "_zod", { value: inner }), "Reflect diff", Reflect.defineProperty(z, "_zod", { value: 5 }));
const w: any = {};
Object.defineProperty(w, "x", { value: 1, writable: true });
Object.defineProperty(w, "x", { value: 2 });
console.log("writable redefine", desc(w, "x"));
const nan: any = {};
Object.defineProperty(nan, "n", { value: NaN });
Object.defineProperty(nan, "n", { value: NaN });
try { Object.defineProperty(nan, "z", { value: 0 }); Object.defineProperty(nan, "z", { value: -0 }); } catch (e) { console.log("-0 redefine:", (e as Error).constructor.name); }
console.log("nan", desc(nan, "n"));

// 4. Non-extensible, sealed and frozen receivers reject fresh keys.
for (const [name, f] of [["preventExtensions", Object.preventExtensions], ["seal", Object.seal], ["freeze", Object.freeze]] as const) {
  const t: any = { k: 1 };
  (f as any)(t);
  try { Object.defineProperty(t, "fresh", { value: 1 }); console.log(name, "defined"); } catch (e) { console.log(name, (e as Error).constructor.name); }
  console.log(name, "Reflect", Reflect.defineProperty(t, "fresh2", { value: 1 }), "k", desc(t, "k"));
}

// 5. Object.defineProperties / Object.create with descriptors.
const multi = Object.defineProperties({} as any, {
  p: { value: "p", enumerable: true },
  q: { get() { return "q"; } },
});
console.log("multi", JSON.stringify(Object.keys(multi)), multi.q, desc(multi, "p"));
const created = Object.create({ base: 1 }, { own: { value: 3, writable: true } });
created.more = 4;
console.log("created", desc(created, "own"), JSON.stringify(Object.keys(created)), created.base);

// 6. Many keys after a hidden key: spilled storage keeps every value.
const wide: any = {};
Object.defineProperty(wide, "_h", { value: "h" });
for (let i = 0; i < 80; i++) wide["k" + i] = i;
let sum = 0;
for (let i = 0; i < 80; i++) sum += wide["k" + i];
console.log("wide", Object.keys(wide).length, sum, wide._h, desc(wide, "k79"));

// 7. Adds that shadow a class method: the shadow bit is kept and set by the
//    probe's key add, so a bound method call sees the own property.
class Parser {
  parse() { return "proto"; }
  check() { return "proto-check"; }
}
const viaCreate: any[] = [];
for (let i = 0; i < 12; i++) {
  const o: any = Object.create(Parser.prototype);
  o.check = () => "own-check";  // a member name: sets the bit
  o.later = i;                  // added on a shadowed object
  if (i % 2 === 0) o.parse = () => "own-parse";
  viaCreate.push(o);
}
console.log("create", viaCreate[0].parse(), viaCreate[1].parse(), viaCreate[3].check(), viaCreate[5].later);
const insts: any[] = [];
for (let i = 0; i < 12; i++) {
  const p: any = new Parser();
  p.tag = i;
  if (i % 3 === 0) p.parse = function () { return "inst-own-" + this.tag; };
  p.after = "a" + i;
  insts.push(p);
}
console.log("inst", insts[0].parse(), insts[1].parse(), insts[3].parse(), insts[4].after, JSON.stringify(Object.keys(insts[3])));
const bag: any = {};
bag.check = 1;
bag.parse = 2;
bag.more = 3;
console.log("bag", JSON.stringify(bag));
