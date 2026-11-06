function result(f: () => any) {
  try { f(); return "ok"; } catch (e: any) { return e.name; }
}
for (let level = 0; level < 3; level++) {
  for (let round = 0; round < 3; round++) {
    const a: any = [3, , 7]; a.named = 11;
    const sym = Symbol.for("array"); a[sym] = 13;
    Object.defineProperty(a, "0", { writable: true, enumerable: false, configurable: true });
    const proto = Object.getPrototypeOf(a);
    if (level === 2) Object.freeze(a);
    else if (level === 1) Object.seal(a);
    else Object.preventExtensions(a);
    const d = Object.getOwnPropertyDescriptor(a, "0")!;
    const l = Object.getOwnPropertyDescriptor(a, "length")!;
    console.log("array", level, round, a.length, d.writable, d.configurable, d.enumerable,
      l.writable, l.configurable, Reflect.set(a, "0", 17), Reflect.set(a, "1", 19),
      Reflect.set(a, "named", 23), Reflect.set(a, sym, 29),
      Object.isFrozen(a), Object.isSealed(a), Object.isExtensible(a),
      Object.getPrototypeOf(a) === proto);
    console.log("mutate", result(() => a.push(31)),
      result(() => Object.defineProperty(a, "2", { value: 37 })),
      result(() => Object.defineProperty(a, "length", { writable: true })));
  }
}
for (const a of [[], [3]]) {
  Object.freeze(a);
  console.log("empty-length", a.length, Object.getOwnPropertyDescriptor(a, "length")!.writable,
    Reflect.set(a, "length", 2), Object.isFrozen(a));
}
for (const a of [new Uint8Array(0), new Uint8Array(2), new Uint32Array(0), new Uint32Array(2)]) {
  console.log("typed-freeze", a.length, result(() => Object.freeze(a)),
    Object.isExtensible(a), Object.isSealed(a), Object.isFrozen(a));
}
for (const a of [new Uint8Array(0), new Uint8Array(2), new Uint32Array(0), new Uint32Array(2)]) {
  console.log("typed-seal", a.length, result(() => Object.seal(a)),
    Object.isExtensible(a), Object.isSealed(a), Object.isFrozen(a), Reflect.set(a, "named", 3));
}

const emptyExt = Object.preventExtensions([]);
console.log("empty-unfrozen", Object.isFrozen(emptyExt), Object.isSealed(emptyExt));
const manual = [3];
Object.defineProperty(manual, "0", { configurable: false, writable: false });
Object.defineProperty(manual, "length", { writable: false });
Object.preventExtensions(manual);
console.log("manual", Object.isFrozen(manual), Object.isSealed(manual));

const grown: any = [3, 5];
const alias = grown;
for (let i = 0; i < 64; i++) grown.push(i);
Object.freeze(alias);
console.log("grown", alias.length, Object.isFrozen(alias),
  Object.getOwnPropertyDescriptor(alias, "0")!.writable,
  Object.getOwnPropertyDescriptor(alias, "length")!.writable,
  Reflect.set(alias, "length", 0), Reflect.set(alias, "0", 71));

const typedManual: any = new Uint8Array(0);
Object.defineProperty(typedManual, "named", { value: 3, writable: false, configurable: false });
Object.preventExtensions(typedManual);
console.log("typed-manual", Object.isExtensible(typedManual), Object.isSealed(typedManual), Object.isFrozen(typedManual));
class FrozenList extends Array {}
const subclass: any = new FrozenList();
subclass.push(7, 11); Object.freeze(subclass);
console.log("subclass", Object.isFrozen(subclass),
  Object.getOwnPropertyDescriptor(subclass, "length")!.writable,
  Reflect.set(subclass, "0", 13), subclass[0]);
