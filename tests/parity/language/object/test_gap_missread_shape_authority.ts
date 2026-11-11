// Same generic sites before and after each semantic change.
function read(o: any) { return o.value; }
function missing(o: any) { return o.missing; }
function call(o: any) { return o.method(); }
function warm(o: any) {
  let sum = 0;
  for (let i = 0; i < 100; i++) sum += read(o) + call(o);
  console.log("warm", sum, missing(o));
}
const holder: any = { value: 3, method() { return this.local; } };
const middle: any = Object.create(holder);
const receiver: any = Object.create(middle);
receiver.local = 5;
warm(receiver);
holder.value = 7;
console.log("value", read(receiver));
middle.value = 11;
console.log("shadow", read(receiver));
delete middle.value;
console.log("delete shadow", read(receiver));
receiver.value = 13;
console.log("own", read(receiver));
delete receiver.value;
console.log("delete own", read(receiver));
holder.missing = 17;
console.log("absent added", missing(receiver));
delete holder.missing;
console.log("absent deleted", missing(receiver));
Object.defineProperty(middle, "value", {
  get() { return this.local * 10; }, configurable: true,
});
console.log("getter", read(receiver));
Object.defineProperty(middle, "value", {
  get() { return this.local * 20; }, configurable: true,
});
console.log("getter replaced", read(receiver));
delete middle.value;
const other: any = { value: 19, method() { return 23; } };
Object.setPrototypeOf(middle, other);
console.log("hop relink", read(receiver), call(receiver));
Object.setPrototypeOf(receiver, holder);
console.log("receiver relink", read(receiver), call(receiver));
let traps = 0;
const proxy: any = new Proxy(other, {
  get(target, key, recv) { traps++; return Reflect.get(target, key, recv); },
});
Object.setPrototypeOf(receiver, proxy);
console.log("proxy", read(receiver), call(receiver), missing(receiver), traps);
Object.setPrototypeOf(receiver, null);
console.log("null", read(receiver), missing(receiver));
const ownGetter: any = { get value() { return 29; } };
console.log("own getter", read(ownGetter));
const nil: any = Object.create({ value: null });
const undef: any = Object.create({ value: undefined });
console.log("nullish data", read(nil), read(undef));
// Wide receiver rotation must preserve each prototype identity.
const wide: any[] = [];
for (let i = 0; i < 40; i++) {
  const p: any = { value: i };
  const o: any = Object.create(p);
  o["shape" + i] = i;
  wide.push(o);
}
let total = 0;
for (let i = 0; i < 400; i++) total += read(wide[i % 40]);
console.log("wide", total);

// A function's own bag is receiver-relative even when outer shapes match.
function readPrototype(f: any) { return f.prototype; }
function readTag(f: any) { return f.tag; }
function F() {}
function G() {}
(F as any).tag = 31;
(G as any).tag = 37;
for (let i = 0; i < 100; i++) {
  readPrototype(F); readPrototype(G); readTag(F); readTag(G);
}
console.log("function bags", readPrototype(F) === F.prototype, readPrototype(G) === G.prototype,
  readTag(F), readTag(G));
(F as any).prototype = { marker: 41 };
console.log("prototype overwrite", readPrototype(F).marker);
(F as any).tag = undefined;
console.log("own undefined", readTag(F));
(G as any).tag = true;
console.log("own boolean", readTag(G) === true);
(G as any).tag = "boxed";
console.log("own string", readTag(G));
const bagObject = { marker: 73 };
(G as any).tag = bagObject;
console.log("own object", readTag(G) === bagObject, readTag(G).marker);
const bagSymbol = Symbol("bag value");
(G as any).tag = bagSymbol;
console.log("own symbol", readTag(G) === bagSymbol);
delete (G as any).tag;
console.log("own deleted", readTag(G));
let getterCalls = 0;
Object.defineProperty(F, "tag", {
  get() { getterCalls++; return 43; }, configurable: true,
});
console.log("function getter", readTag(F), readTag(F), getterCalls);
Object.defineProperty(F, "tag", { value: 47, writable: true, configurable: true });
console.log("function data", readTag(F));
Object.setPrototypeOf(G, { tag: 53 });
console.log("function inherited", readTag(G));
class A { static tag = 59; }
class B extends A { static tag = 61; }
for (let i = 0; i < 100; i++) { readPrototype(A); readPrototype(B); readTag(A); readTag(B); }
console.log("class bags", readPrototype(A) === A.prototype, readPrototype(B) === B.prototype,
  readTag(A), readTag(B));
delete (B as any).tag;
console.log("class unshadow", readTag(B));
Object.defineProperty(A, "tag", { get() { return this === B ? 67 : 71; }, configurable: true });
console.log("class getter", readTag(A), readTag(B));

function parentOf(o: any) { return Object.getPrototypeOf(o); }
const reflective = Object.create(holder);
for (let i = 0; i < 100; i++) parentOf(reflective);
console.log("shape link", parentOf(reflective) === holder);
Object.setPrototypeOf(reflective, other);
console.log("shape relink", parentOf(reflective) === other);
Object.setPrototypeOf(reflective, null);
console.log("shape null", parentOf(reflective) === null);
Object.setPrototypeOf(reflective, Object.prototype);
console.log("shape default", parentOf(reflective) === Object.prototype);
let prototypeTraps = 0;
const reflectiveProxy = new Proxy(reflective, {
  getPrototypeOf() { prototypeTraps++; return holder; },
});
for (let i = 0; i < 100; i++) parentOf(reflectiveProxy);
console.log("prototype trap", parentOf(reflectiveProxy) === holder, prototypeTraps);
console.log("weak prototypes", parentOf(new WeakMap()) === WeakMap.prototype,
  parentOf(new WeakSet()) === WeakSet.prototype);
console.log("typed prototypes", parentOf(Uint8Array.prototype) === parentOf(Int8Array.prototype));
