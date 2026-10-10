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

// Reflection uses CreateDataProperty even when Object.prototype has setters.
let intercepted = 0;
Object.defineProperty(Object.prototype, "recordKey", {
  set(v: any) { intercepted++; }, configurable: true
});
const pollutedSource: any = Object.create(null);
Object.defineProperty(pollutedSource, "recordKey", { value: 47, enumerable: true });
Object.defineProperty(pollutedSource, "__proto__", { value: 53, enumerable: true });
const reflection: any = Object.getOwnPropertyDescriptors(pollutedSource);
delete (Object.prototype as any).recordKey;
console.log("reflection-create", intercepted, Object.hasOwn(reflection, "recordKey"),
  reflection.recordKey.value, Object.hasOwn(reflection, "__proto__"),
  reflection.__proto__.value, Object.getPrototypeOf(reflection) === Object.prototype);
// Decode attributes from frozen and null-prototype bags; own data may be undefined.
const nullBag: any = Object.create(null);
nullBag.value = undefined; nullBag.enumerable = true;
const attrs: any = Object.create(null);
Object.defineProperties(attrs, { missing: Object.freeze(nullBag), data: Object.freeze({ value: 67, writable: true }) });
console.log("bags", Object.hasOwn(attrs, "missing"), attrs.missing,
  Object.keys(attrs).join(","), attrs.data, Object.getOwnPropertyDescriptor(attrs, "data")!.writable);
// A descriptor getter can revoke the collected bag's shape proof.
const changing: any = {
  first: { get value() {
    Object.defineProperty(changing, "later", { enumerable: false });
    changing.last = { value: 103, enumerable: true };
    changing.added = { value: 107, enumerable: true };
    return 101;
  }, enumerable: true },
  later: { value: 109, enumerable: true },
  last: { value: 113, enumerable: true }
};
const changed: any = Object.create(null, changing);
console.log("revalidated", Object.keys(changed).join(","), changed.first, changed.last,
  Object.hasOwn(changed, "later"), Object.hasOwn(changed, "added"));
// Replacing a slot's value leaves its ShapeId intact; read the current value.
const replacing: any = {
  first: { get value() { replacing.later = { value: 127, enumerable: true }; return 131; }, enumerable: true },
  later: { value: 137, enumerable: true }
};
const replaced: any = Object.create(null, replacing);
console.log("current-slot", replaced.first, replaced.later);
const redefined: any = {};
Object.defineProperty(redefined, "fixed", { value: 139, writable: true });
let rejected = "ok";
try { Object.defineProperty(redefined, "fixed", { value: 149, writable: true, enumerable: true, configurable: true }); }
catch (e: any) { rejected = e.name; }
let getterCalls = 0;
Object.defineProperty(redefined, "replace", { get() { getterCalls++; return 151; }, configurable: true });
Object.defineProperty(redefined, "replace", { value: 157, writable: true, enumerable: true, configurable: true });
console.log("complete-data", rejected, redefined.fixed, redefined.replace, getterCalls,
  Object.keys(redefined).join(","));

// A reflection result owns its properties independently of a source whose
// key list was edited; later source deletions/appends cannot rename entries.
const editedSource: any = { a: 1, b: 2, c: 3, e: 5 };
delete editedSource.b;
const savedDescriptors: any = Object.getOwnPropertyDescriptors(editedSource);
delete editedSource.a;
editedSource.d = 4;
const savedCopy: any = Object.create(null, savedDescriptors);
console.log("snapshot-owned", Object.keys(savedCopy).join(","), savedCopy.a, savedCopy.c, savedCopy.e, savedCopy.d);
