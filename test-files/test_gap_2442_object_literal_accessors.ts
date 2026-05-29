// #2442: getters/setters defined in an OBJECT LITERAL (`{ get k(){}, set k(v){} }`)
// must be installed as accessor descriptors — reads invoke the getter, writes
// invoke the setter. Pre-fix the object-literal lowering dropped `GetterProp` /
// `SetterProp` members (the `_ => {}` catch-all), so the property read as
// `undefined` and the accessor bodies never ran.
//
// Class accessors (`class { get x(){} }`) and
// `Object.defineProperty(obj, k, { get })` already worked; this locks in the
// object-literal syntax specifically.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

// (1) get + set on the same key, with a side-effect log proving both bodies run.
const log: string[] = [];
const g = {
  get v() {
    log.push("get");
    return 5;
  },
  set v(x: number) {
    log.push("set" + x);
  },
};
console.log("read:", g.v); // 5
g.v = 9; // setter runs
console.log("log:", JSON.stringify(log)); // ["get","set9"]

// (2) accessor backed by a captured local (closure capture).
let store = 0;
const h = {
  a: 1,
  get x() {
    return store;
  },
  set x(val: number) {
    store = val;
  },
  b: 2,
};
console.log("data:", h.a, h.b); // 1 2
h.x = 42;
console.log("accessor:", h.x); // 42
console.log("store:", store); // 42

// (3) getter that reads `this` — must bind to the receiver object.
const counter = {
  _n: 10,
  get doubled() {
    return this._n * 2;
  },
};
console.log("this-getter:", counter.doubled); // 20
counter._n = 7;
console.log("this-getter2:", counter.doubled); // 14

// (4) key ordering: the accessor key keeps its source position.
console.log("keys:", JSON.stringify(Object.keys(h))); // ["a","x","b"]

// (5) getOwnPropertyDescriptor reflects the accessor (enumerable + configurable).
const d = {
  get z() {
    return 7;
  },
  set z(v: number) {},
};
const desc = Object.getOwnPropertyDescriptor(d, "z")!;
console.log(
  "descriptor:",
  typeof desc.get,
  typeof desc.set,
  desc.enumerable,
  desc.configurable,
); // function function true true

// (6) for-in enumerates the accessor key.
const f = {
  p: 1,
  get q() {
    return 2;
  },
};
const seen: string[] = [];
for (const k in f) seen.push(k);
console.log("for-in:", JSON.stringify(seen)); // ["p","q"]
