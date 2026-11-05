"use strict";

// Multi-hop array prototypes must preserve Receiver and stop at the first own
// property, including undefined values and writable shadows of readonly data.
const reads: string[] = [];
const far: any = [];
const near: any = [];
const receiver: any = ["receiver"];
Object.defineProperty(far, "2", {
  configurable: true,
  get() { reads.push(`get:${this === receiver}`); return this[0]; },
});
Object.setPrototypeOf(near, far);
Object.setPrototypeOf(receiver, near);
console.log("receiver", receiver[2], Array.prototype.at.call(receiver, 2), reads.join("|"));
receiver.length = 3;
reads.length = 0;
console.log("join", Array.prototype.join.call(receiver, ","), reads.join("|"));

const readonly: any = {};
Object.defineProperty(readonly, "4", { value: "readonly", writable: false });
const shadow: any = [];
shadow[4] = undefined;
Object.setPrototypeOf(shadow, readonly);
const child: any = [];
Object.setPrototypeOf(child, shadow);
console.log("shadow-before", child[4], 4 in child);
child[4] = "written";
console.log("shadow-after", child[4], Object.hasOwn(child, 4), shadow[4]);

// A grown prototype is still the same chain node, even when its old allocation
// became a forwarding stub after the child captured it.
const grown: any = [];
const grownChild: any = [];
Object.setPrototypeOf(grownChild, grown);
for (let i = 0; i < 100; i++) grown.push(i);
console.log("grown", grownChild[99], 99 in grownChild, 100 in grownChild);

// Interleave arrays and ordinary objects before a Proxy to check that its
// traps still observe the original array and run once per internal operation.
const traps: string[] = [];
let original: any;
const proxy = new Proxy({}, {
  get(_target, key, recv) {
    if (key === "1") { traps.push(`get:${recv === original}`); return "one"; }
    return undefined;
  },
  has(_target, key) {
    if (key === "1" || key === "7") traps.push(`has:${String(key)}`);
    return key === "1";
  },
});
const objectHop = Object.create(proxy);
const arrayHop: any = [];
Object.setPrototypeOf(arrayHop, objectHop);
original = [0, , 2];
Object.setPrototypeOf(original, arrayHop);
console.log("proxy-get", original[1], traps.join("|"));
traps.length = 0;
console.log("proxy-has", 1 in original, traps.join("|"));
traps.length = 0;
console.log("proxy-indexOf", Array.prototype.indexOf.call(original, "one"), traps.join("|"));
traps.length = 0;
console.log("proxy-missing", 7 in original, "7" in original, traps.join("|"));

// An inherited Proxy get trap may itself perform an unrelated inherited read.
// The outer Receiver must not leak into that nested lookup.
const innerProto = { get marker() { return this.name; } };
const inner = Object.create(innerProto);
inner.name = "inner";
const nestedProxy = new Proxy({}, {
  get(_target, key, recv) {
    return `${recv === nested}:${inner.marker}`;
  },
});
const nested: any = [];
Object.setPrototypeOf(nested, Object.create(nestedProxy));
console.log("nested-trap", nested[1]);

const targetGetter = { get 1() { return this.name; } };
const noTrap = new Proxy(new Proxy(targetGetter, {}), {});
const getterChild: any = [];
getterChild.name = "array";
Object.setPrototypeOf(getterChild, noTrap);
console.log("proxy-getter", getterChild[1], Reflect.get(noTrap, "1", { name: "reflect" }));
let reflected: any;
const reflectProxy = new Proxy({}, {
  get(_target, _key, recv) { return recv === reflected; },
});
reflected = {};
console.log("reflect-receiver", Reflect.get(reflectProxy, "x", reflected));
