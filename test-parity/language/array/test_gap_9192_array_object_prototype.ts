// #9192: `Object.setPrototypeOf(array, <a plain object>)` — the array must
// inherit from that object. Perry recorded the retarget (paying the
// process-wide index deoptimisation for it) and then declined to consult it:
// `array_custom_array_prototype` accepted a recorded `[[Prototype]]` only when
// the prototype was ITSELF a `GC_TYPE_ARRAY`, and the named-property fallback
// (`array_prototype_property_value`) hardcoded `Array.prototype`. So a
// retargeted array inherited NOTHING from its new prototype while still
// inheriting everything from the old one — silent wrong values, no crash.
//
// The one fixture that covered prototype retargeting on an array
// (test_gap_typed_arrays.ts) uses an ARRAY as the prototype, the single shape
// that already worked. This covers the shapes that did not: a plain object, the
// ES5 `Object.create(Array.prototype)` subclass idiom, a deeper chain, an
// accessor-bearing prototype, `null`, and the three ways to install one
// (`Object.setPrototypeOf`, `__proto__`, `Reflect.setPrototypeOf`).
//
// Run: node --experimental-strip-types test_gap_9192_array_object_prototype.ts

// --- a plain object prototype: indexed AND named inheritance ---
const protoA: any = { 7: "inherited", foo: "bar" };
const a: any = [1, 2, 3];
Object.setPrototypeOf(a, protoA);

console.log("A elem:", a[7], a["7"], a[2]);
console.log("A named:", a.foo);
console.log("A in:", 7 in a, "7" in a, "foo" in a, 2 in a, 9 in a);
console.log(
  "A hasOwn:",
  Object.prototype.hasOwnProperty.call(a, 7),
  Object.prototype.hasOwnProperty.call(a, "foo"),
  Object.prototype.hasOwnProperty.call(a, 0),
);
console.log("A length/isArray:", a.length, Array.isArray(a));
console.log("A keys:", Object.keys(a).join(","));
console.log("A ownNames:", Object.getOwnPropertyNames(a).join(","));
console.log("A json:", JSON.stringify(a));
console.log("A proto identity:", Object.getPrototypeOf(a) === protoA, a.__proto__ === protoA);
console.log("A ownDesc:", Object.getOwnPropertyDescriptor(a, 7), Object.getOwnPropertyDescriptor(a, "foo"));

// `Array.prototype` is no longer on the chain, so its methods are gone.
console.log("A methods gone:", typeof a.map, typeof a.push, typeof a.join);
// ...but a borrowed one still works on the array-like receiver.
console.log("A borrowed:", Array.prototype.join.call(a, "-"));

// An own write shadows the inherited property without disturbing the prototype.
a[7] = "own";
a.foo = "mine";
console.log("A shadowed:", a[7], a.foo, protoA[7], protoA.foo);
console.log("A hasOwn after write:", Object.prototype.hasOwnProperty.call(a, 7));

// --- the ES5 array-subclass idiom ---
function MyList(this: any) {}
MyList.prototype = Object.create(Array.prototype);
MyList.prototype.tag = "mylist";
MyList.prototype.first = function (this: any) {
  return this[0];
};
MyList.prototype[5] = "fifth";

const b: any = [10, 20];
Object.setPrototypeOf(b, MyList.prototype);
console.log("B inherited:", b.tag, b.first(), b[5], 5 in b);
console.log("B instanceof:", b instanceof (MyList as any), b instanceof Array, Array.isArray(b));
// `Array.prototype` is still on the chain here, so its methods keep working.
console.log("B array methods:", b.map((x: number) => x + 1).join(","), b.join("-"));
b.push(30);
console.log("B after push:", b.length, b[2], JSON.stringify(b), b.slice().join(","));
console.log("B keys:", Object.keys(b).join(","));
console.log("B proto identity:", Object.getPrototypeOf(b) === MyList.prototype);

// --- a deeper chain that still reaches Array.prototype ---
const mid: any = Object.create(Array.prototype);
mid.mid = "M";
const top: any = Object.create(mid);
top.top = "T";
top[8] = "eight";
const j: any = [0];
Object.setPrototypeOf(j, top);
console.log("J chain:", j.top, j.mid, j[8], 8 in j, typeof j.map);
console.log("J map:", j.map((x: number) => x + 1).join(","));

// --- an accessor on the prototype, at an index and at a name ---
const protoG: any = {};
let getterCalls = 0;
let getterThisIsG = false;
Object.defineProperty(protoG, "9", {
  configurable: true,
  get(this: any) {
    getterCalls++;
    getterThisIsG = this === g;
    return "acc9";
  },
});
Object.defineProperty(protoG, "named", {
  configurable: true,
  get() {
    return "accNamed";
  },
});
const g: any = [1];
Object.setPrototypeOf(g, protoG);
console.log("G accessor:", g[9], getterCalls, getterThisIsG, g.named);

// --- a null prototype inherits nothing at all ---
const c: any = [1, 2];
Object.setPrototypeOf(c, null);
console.log("C basics:", c[0], c.length, Array.isArray(c), Object.getPrototypeOf(c));
console.log("C nothing inherited:", typeof c.map, "toString" in c, 0 in c, 5 in c);
console.log("C keys/json:", Object.keys(c).join(","), JSON.stringify(c));
c[3] = 7;
console.log("C after write:", c.length, c[3]);

// --- restoring Array.prototype restores the default behaviour ---
const h: any = [1, 2];
Object.setPrototypeOf(h, { 5: "tmp" });
console.log("H retargeted:", h[5]);
Object.setPrototypeOf(h, Array.prototype);
console.log("H restored:", h[5], h.map((x: number) => x * 3).join(","), Object.getPrototypeOf(h) === Array.prototype);

// --- an ARRAY prototype (the shape that already worked) must keep working,
//     including its NAMED properties, which did NOT work before #9192.
const protoI: any = [];
protoI[6] = "protoSix";
protoI.tagged = "arrProto";
const i2: any = [1, 2];
Object.setPrototypeOf(i2, protoI);
console.log("I array proto:", i2[6], 6 in i2, i2.tagged, i2.length, typeof i2.map);

// --- `__proto__` and `Reflect.setPrototypeOf` install the same link ---
const d: any = [1];
const protoD: any = { 3: "d3", zap: "zop" };
d.__proto__ = protoD;
console.log("D __proto__:", d[3], d.zap, 3 in d, Object.getPrototypeOf(d) === protoD);

const e: any = [1];
const protoE: any = { 4: "e4", zip: "zup" };
console.log("E reflect set:", Reflect.setPrototypeOf(e, protoE));
console.log("E reflect read:", e[4], e.zip, Reflect.has(e, "zip"), Reflect.has(e, "4"), Reflect.get(e, "4"));
console.log("E ownKeys:", Reflect.ownKeys(e).map(String).join(","));

// --- a hole reads through the replaced chain ---
const k: any = [0, , 2];
Object.setPrototypeOf(k, { 1: "holeFill" });
console.log("K hole:", k[1], 1 in k);

// --- `Object.create(Array.prototype)` itself is NOT an array ---
const f: any = Object.create(Array.prototype);
f[0] = "x";
f.length = 1;
console.log("F not an array:", Array.isArray(f), f.length, f.join("-"), f[0]);
