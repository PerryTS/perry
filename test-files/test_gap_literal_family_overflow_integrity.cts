// Sloppy script: family overflow owns attributes of fixed and dynamic keys.
function literalHolder() { return { a: 1 }; }
function integrityHolder() {
  const o: any = literalHolder();
  o.reserved = 2;
  return o;
}
function strictReserved(o: any) { "use strict"; o.reserved = 99; }
function strictDynamic(o: any, key: string) { "use strict"; o[key] = 99; }
function verdict(f: () => any) {
  try { f(); return "ok"; } catch (e: any) { return e.name; }
}
for (const n of [3, 1500]) {
  const o: any = integrityHolder();
  for (let i = 0; i < n; i++) o["dynamic_" + i] = i;
  const outside = ["out", "side"].join("");
  Object.defineProperty(o, "reserved", { value: 7, writable: false, configurable: true });
  Object.defineProperty(o, outside, { value: 11, writable: false, configurable: true });
  console.log("before", n, Object.getOwnPropertyDescriptor(o, "reserved")!.writable,
    Object.getOwnPropertyDescriptor(o, outside)!.writable);
  o.reserved = 13;
  o[outside] = 17;
  console.log("sloppy", n, o.reserved, o[outside]);
  console.log("strict", n, verdict(() => strictReserved(o)), verdict(() => strictDynamic(o, outside)));
  console.log("reflect", n, Reflect.set(o, "reserved", 19), Reflect.set(o, outside, 23),
    o.reserved, o[outside]);
}
for (let level = 0; level < 3; level++) {
  const o: any = integrityHolder();
  o[["dynamic", level].join("_")] = level;
  const data = Symbol("data");
  const access = Symbol("access");
  let state = 29;
  const get = () => state;
  const set = (v: number) => { state = v; };
  Object.defineProperty(o, data, { value: 31, writable: true, enumerable: true, configurable: true });
  Object.defineProperty(o, access, { get, set, enumerable: true, configurable: true });
  if (level === 2) Object.freeze(o);
  else if (level === 1) Object.seal(o);
  else Object.preventExtensions(o);
  const d = Object.getOwnPropertyDescriptor(o, data)!;
  const a = Object.getOwnPropertyDescriptor(o, access)!;
  console.log("symbol", level, d.writable, d.configurable, a.configurable,
    Object.hasOwn(a, "writable"), a.get === get, a.set === set,
    Object.isExtensible(o), Object.isSealed(o), Object.isFrozen(o));
  console.log("symbol-set", level, Reflect.set(o, data, 37), Reflect.set(o, access, 41),
    o[data], o[access], Object.getOwnPropertyDescriptor(o, access)!.configurable);
}

