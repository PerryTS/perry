// #10753: computed string-key reads through prototypes, accessors, Proxies and
// class instances, each warmed first so a shape answer or a filed absent
// verdict would be in place, then changed under it.

function readAll(o: any, keys: string[]): string {
  return keys.map((k) => String(o[k])).join(",");
}
function warm(o: any, keys: string[]): string {
  let last = "";
  for (let i = 0; i < 40; i++) last = readAll(o, keys);
  return last;
}

// Prototype-inherited data, and a key that appears on the prototype later.
const proto: any = { inh: 7 };
const child: any = Object.create(proto);
child.own = 1;
const CK = ["own", "inh", "none"];
console.log("proto0", warm(child, CK));
proto.none = 9;
console.log("proto1", readAll(child, CK));
child.inh = 70;
console.log("proto2", readAll(child, CK));
delete proto.none;
console.log("proto3", readAll(child, CK));

// Own getters, and an accessor added to Object.prototype after an absent
// verdict for the same key was filed.
let calls = 0;
const g: any = {
  get v() {
    calls++;
    return 42;
  },
  w: 1,
};
const GK = ["v", "w", "q"];
console.log("getter0", warm(g, GK), calls);
Object.defineProperty(Object.prototype, "q", {
  get() {
    return "protoGetter:" + typeof this;
  },
  configurable: true,
});
console.log("getter1", readAll(g, GK), calls);
delete (Object.prototype as any).q;
console.log("getter2", readAll(g, GK));

// A getter on a prototype reading the receiver.
const gp: any = Object.create({
  get pv() {
    return this.w2 * 2;
  },
});
gp.w2 = 5;
console.log("protogetter0", warm(gp, ["pv", "w2", "zz"]));
gp.w2 = 6;
console.log("protogetter1", readAll(gp, ["pv", "w2", "zz"]));

// A data property turned into an accessor on the receiver itself.
const d: any = { a: 1, b: 2 };
console.log("redefine0", warm(d, ["a", "b", "c"]));
Object.defineProperty(d, "a", { get: () => "acc" });
console.log("redefine1", readAll(d, ["a", "b", "c"]));

// Proxies: every computed read goes to the trap.
const target: any = { a: 1 };
const p: any = new Proxy(target, {
  get(t, k) {
    return typeof k === "string" ? "P:" + k + ":" + String(t[k]) : undefined;
  },
});
console.log("proxy0", warm(p, ["a", "zz"]));
target.zz = 2;
console.log("proxy1", readAll(p, ["a", "zz"]));
// An object whose prototype is a Proxy.
const viaProxy: any = Object.create(
  new Proxy({}, { get: (_t, k) => (typeof k === "string" ? "trap:" + k : undefined) }),
);
viaProxy.own = 3;
console.log("protoproxy", warm(viaProxy, ["own", "missing"]));

// setPrototypeOf after warming.
const sp: any = { a: 1 };
console.log("setproto0", warm(sp, ["a", "b"]));
Object.setPrototypeOf(sp, { b: "fromNewProto" });
console.log("setproto1", readAll(sp, ["a", "b"]));
Object.setPrototypeOf(sp, null);
console.log("setproto2", readAll(sp, ["a", "b", "toString"]));

// Class instances: fields, methods, accessors and absent keys.
class C {
  a = 1;
  m() {
    return "m";
  }
  get g() {
    return "g";
  }
}
class D extends C {
  b = 2;
}
const inst: any = new D();
const IK = ["a", "b", "g", "m", "h", "constructor"];
for (let r = 0; r < 3; r++) console.log("class", r, IK.map((k) => typeof inst[k]).join(","));
(C.prototype as any).h = "late";
console.log("class-late", String(inst["h"]));

// Frozen and sealed receivers read the same.
const fr: any = Object.freeze({ a: 1, b: 2 });
console.log("frozen", warm(fr, ["a", "b", "c"]));

// Reads under allocation pressure: collections move receivers and keys
// between warm reads and the next answer.
const live: any[] = [];
const PK = ["k0", "k1", "k2", "absent0", "absent1"];
const holder: any = { k0: 0, k1: 1, k2: 2 };
let acc = 0;
for (let i = 0; i < 20000; i++) {
  live.push({ i, s: "s" + i, arr: [i, i + 1] });
  if (live.length > 500) live.splice(0, 250);
  const v = holder[PK[i % 5]];
  acc += v === undefined ? 1000 : v;
}
console.log("pressure", acc, live.length);
