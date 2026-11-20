function strictWrite(o: any) { "use strict"; o.x = 99; }
function strictAdd(o: any) { "use strict"; o.y = 99; }
function result(f: () => any) {
  try { f(); return "ok"; } catch (e: any) { return e.name; }
}
for (let level = 0; level < 3; level++) {
  const a: any = { x: 3 }, b: any = { x: 5 }, c: any = { x: 7 };
  const proto = Object.getPrototypeOf(a);
  for (const o of [a, b, c]) {
    const out = level === 2 ? Object.freeze(o) : level === 1 ? Object.seal(o) : Object.preventExtensions(o);
    console.log("level", level, out === o, Object.isExtensible(o), Object.isSealed(o), Object.isFrozen(o),
      Object.getOwnPropertyDescriptor(o, "x")!.writable, Object.getOwnPropertyDescriptor(o, "x")!.configurable,
      Object.getPrototypeOf(o) === proto);
    console.log("writes", result(() => strictWrite(o)), result(() => strictAdd(o)),
      Reflect.set(o, "x", 11), Reflect.set(o, "y", 13), o.x, Object.hasOwn(o, "y"));
    console.log("define", result(() => Object.defineProperty(o, "x", { value: 17 })),
      result(() => Object.defineProperty(o, "x", { configurable: true })),
      result(() => Object.defineProperty(o, "y", { value: 19 })));
    Object.freeze(o);
    console.log("upgrade", Object.isFrozen(o), Object.freeze(o) === o);
  }
}
const empty: any = Object.preventExtensions({});
console.log("empty", Object.isSealed(empty), Object.isFrozen(empty));
const sloppy: any = Object.freeze({ x: 23 });
new Function("object", "object.x = 29; object.y = 31; return object.x;")(sloppy);
console.log("sloppy", sloppy.x, Object.hasOwn(sloppy, "y"));
const symbol = Symbol("own");
let value = 37;
const get = () => value;
const set = (v: number) => { value = v; };
for (let i = 0; i < 3; i++) {
  const o: any = { x: i };
  Object.defineProperty(o, "access", { get, set, enumerable: false, configurable: true });
  Object.defineProperty(o, symbol, { get, set, enumerable: true, configurable: true });
  o[Symbol.for("data")] = 41;
  Object.freeze(o);
  o.access = 43; o[symbol] = 47;
  const d = Object.getOwnPropertyDescriptor(o, "access")!;
  const s = Object.getOwnPropertyDescriptor(o, symbol)!;
  console.log("access", d.get === get, d.set === set, d.configurable, d.enumerable,
    s.get === get, s.set === set, s.configurable, o.access, o[symbol],
    Reflect.set(o, Symbol.for("data"), 53), Object.isFrozen(o));
}
const sharedProto = { inherited: 59 };
for (let i = 0; i < 3; i++) {
  const o: any = Object.create(sharedProto);
  o.x = i;
  Object.freeze(o);
  console.log("proto", Object.getPrototypeOf(o) === sharedProto, o.inherited,
    result(() => Object.setPrototypeOf(o, {})), Object.setPrototypeOf(o, sharedProto) === o);
}

class PrivateFreeze { #x = 5; add() { this.#x++; } get() { return this.#x; } }
const privateFrozen = Object.freeze(new PrivateFreeze());
privateFrozen.add();
console.log("private", privateFrozen.get());
