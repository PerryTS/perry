
// Source and destination exotic families keep their normal internal methods.
const sym = Symbol("copy");
const arr: any = [3, , 7]; arr.named = 11; arr[sym] = 13;
const typed: any = new Uint8Array([17, 19]); typed.named = 23; typed[sym] = 29;
const callable = (x: number) => x + 1;
const fn: any = callable; fn.named = 31; fn[sym] = 37;
class RecordClass { value = 41; get derived() { return this.value + 1; } }
const instance: any = new RecordClass(); instance[sym] = 43;
for (const src of [arr, typed, fn, instance]) {
  const assigned: any = Object.assign({}, src);
  const clone: any = Object.create(Object.getPrototypeOf(src), Object.getOwnPropertyDescriptors(src));
  console.log("exotic", Object.keys(assigned).join(","), assigned[sym],
    Object.getOwnPropertyNames(clone).join(","), Object.getOwnPropertySymbols(clone).length);
}
const targetArray: any = [0]; Object.assign(targetArray, { "2": 47, named: 53 });
const targetTyped: any = new Uint8Array([0, 0]); Object.assign(targetTyped, { "1": 59, named: 61 });
Object.assign(fn, { added: 67 }); Object.assign(instance, { value: 71 });
console.log("targets", targetArray.length, targetArray[2], targetArray.named,
  targetTyped[1], targetTyped.named, fn.added, instance.derived);
const log: string[] = [];
const psym = Symbol("proxy");
const sourceBase: any = { "10": 10, "2": 2, a: 3 }; sourceBase[psym] = 5;
const source: any = new Proxy(sourceBase, {
  ownKeys(t) { log.push("keys"); return [psym, "a", "10", "2"]; },
  getOwnPropertyDescriptor(t, k) { log.push("desc " + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); },
  get(t, k, r) { log.push("get " + String(k)); return Reflect.get(t, k, r); }
});
const target: any = new Proxy({}, {
  set(t, k, v, r) { log.push("set " + String(k)); return Reflect.set(t, k, v, r); },
  defineProperty(t, k, d) { log.push("define " + String(k)); return Reflect.defineProperty(t, k, d); }
});
Object.assign(target, source);
console.log("proxy-assign", log.join("|")); log.length = 0;
const bag: any = Object.getOwnPropertyDescriptors(source);
console.log("proxy-descriptors", log.join("|"), Object.keys(bag).join(","), bag[psym].value);
for (const target of [Object.freeze({ a: 1 }), Object.seal({ a: 2 }), Object.preventExtensions({ a: 3 })]) {
  let ok = "ok";
  try { Object.assign(target, { a: 79, b: 83 }); } catch (e: any) { ok = e.name; }
  console.log("restricted-target", ok, target.a, Object.hasOwn(target, "b"));
}
