// Source descriptors must survive copying; descriptor bags remain mutable values.
const sym = Symbol("visible");
const hidden = Symbol("hidden");
const proto: any = { inherited: 91 };
const src: any = Object.create(proto);
let gets = 0;
let stored = 8;
Object.defineProperties(src, {
  "10": { value: 10, writable: false, enumerable: true, configurable: false },
  "2": { value: 2, writable: true, enumerable: true, configurable: true },
  label: { value: "record", writable: false, enumerable: false, configurable: true },
  accessor: { get() { gets++; return stored; }, set(v: number) { stored = v; },
    enumerable: true, configurable: true },
  "__proto__": { value: 77, writable: true, enumerable: true, configurable: true }
});
Object.defineProperty(src, "__proto__", { value: 77, writable: true, enumerable: true, configurable: true });
Object.defineProperty(src, sym, { value: 31, writable: false, enumerable: true, configurable: true });
Object.defineProperty(src, hidden, { get() { gets++; return 41; }, enumerable: false, configurable: false });
const bag: any = Object.getOwnPropertyDescriptors(src);
const created: any = Object.create(proto, bag);
const defined: any = Object.defineProperties(Object.create(proto), bag);
function inspect(label: string, obj: any) {
  console.log(label, Object.getOwnPropertyNames(obj).join(","), Object.getOwnPropertySymbols(obj).length,
    Object.getPrototypeOf(obj) === proto, Object.hasOwn(obj, "inherited"));
  for (const key of ["2", "10", "label", "accessor", "__proto__"]) {
    const d: any = Object.getOwnPropertyDescriptor(obj, key);
    console.log(key, d.writable, d.enumerable, d.configurable, d.value, typeof d.get, typeof d.set);
  }
  const sd: any = Object.getOwnPropertyDescriptor(obj, sym);
  const hd: any = Object.getOwnPropertyDescriptor(obj, hidden);
  console.log("symbols", sd.value, sd.writable, hd.enumerable, hd.configurable, hd.get === bag[hidden].get);
}
inspect("source", src);
inspect("created", created);
inspect("defined", defined);
console.log("getters-before-read", gets, bag.accessor.get === Object.getOwnPropertyDescriptor(src, "accessor")!.get);
created.accessor = 19;
console.log("receiver-access", defined.accessor, gets, stored);
// A bag's ordinary edits must win over any facts of its original source.
bag["2"].value = 202;
delete bag["10"];
bag.extra = { value: 303, enumerable: true };
const edited: any = Object.create(null, bag);
console.log("edited", edited["2"], Object.hasOwn(edited, "10"), edited.extra,
  Object.getPrototypeOf(edited) === null, Object.isExtensible(edited));
for (const source of [Object.freeze({ k: 1 }), Object.seal({ k: 2 }), Object.preventExtensions({ k: 3 })]) {
  const copy: any = Object.create(null, Object.getOwnPropertyDescriptors(source));
  const d: any = Object.getOwnPropertyDescriptor(copy, "k");
  console.log("restricted-source", copy.k, d.writable, d.configurable, Object.isExtensible(copy));
  copy.extra = 4;
  console.log("extra", copy.extra);
}
