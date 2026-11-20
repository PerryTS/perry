// A constructor that defines receiver keys with `Object.defineProperty(this,
// …)` / `Object.defineProperties(this, …)` in a program that may replace a
// member of `Object` (a computed-key store onto the global object, as a DOM
// runtime's window install does): the define is guarded by the namespace
// test, and the instance keeps its class layout either way. Every observable
// result (own keys in order, attributes, reads of own, inherited and absent
// keys, `in`, deletes, later adds) matches whether the original builtin or a
// replacement runs, including a replacement installed after the first
// instances were built that sees the half-built receiver.

const g: any = globalThis;
for (const [k, v] of Object.entries({ RF4Probe: 1, RF4Other: 2 })) g[k] = v;

let nextId = 0;
class Base {
  isBase: boolean;
  uuid: string;
  constructor() {
    this.isBase = true;
    Object.defineProperty(this, "id", { value: nextId++ });
    this.uuid = "u" + nextId;
    const position = { x: 1 };
    Object.defineProperties(this, {
      position: { configurable: true, enumerable: true, value: position },
      hidden: { value: 7 },
    });
  }
  get kind(): string {
    return "base";
  }
}
(Base.prototype as any).inherited = "proto";
class Leaf extends Base {
  isLeaf: boolean;
  constructor() {
    super();
    this.isLeaf = true;
  }
}

function show(tag: string, o: any): void {
  const d = (k: string) => {
    const x = Object.getOwnPropertyDescriptor(o, k);
    return x ? `${x.writable ? "w" : "-"}${x.enumerable ? "e" : "-"}${x.configurable ? "c" : "-"}` : "none";
  };
  console.log(
    tag,
    JSON.stringify(Object.keys(o)),
    JSON.stringify(Object.getOwnPropertyNames(o)),
    d("id"),
    d("position"),
    d("hidden"),
    typeof o.id,
    o.position && o.position.x,
    o.hidden,
    o.inherited,
    o.kind,
    o.absent,
    "id" in o,
    o.isLeaf,
  );
}

const a = new Leaf();
const b = new Base();
show("leaf", a);
show("base", b);

// Reads at one site over many instances of both classes.
let sum = 0;
const objs: any[] = [];
for (let i = 0; i < 50; i++) objs.push(i % 2 ? new Leaf() : new Base());
for (let r = 0; r < 20; r++) {
  for (const o of objs) {
    sum += o.id + o.hidden + (o.isLeaf ? 1 : 0) + (o.absent === undefined ? 1 : 0) + o.position.x;
  }
}
console.log("sum", sum);

// The attributes hold: a non-writable id ignores (strict: rejects) writes.
try {
  (a as any).id = 99;
} catch (e: any) {
  console.log("write id", e.constructor.name);
}
console.log("id after write", typeof a.id, a.id === 0);
(a as any).later = 1;
delete (a as any).position;
show("leaf mutated", a);

// A replacement `Object.defineProperty` that sees the half-built receiver,
// installed after the first instances: it defines a different key first.
const original = Object.defineProperty;
const seen: string[] = [];
(Object as any).defineProperty = function (o: any, k: any, d: any) {
  seen.push(String(k) + ":" + JSON.stringify(Object.keys(o)));
  original(o, "extra", { value: "x", enumerable: true });
  return original(o, k, d);
};
const c = new Leaf();
show("patched", c);
console.log("seen", seen.join(" "));
(Object as any).defineProperty = original;
(Object as any).defineProperties = function (o: any, _p: any) {
  seen.push("props");
  return o;
};
const d2 = new Leaf();
show("patched props", d2);
console.log("seen", seen.join(" "));
let sum2 = 0;
for (const o of [a, b, c, d2, ...objs]) sum2 += (o.position ? o.position.x : 10) + (o.hidden ?? 100);
console.log("sum2", sum2);
