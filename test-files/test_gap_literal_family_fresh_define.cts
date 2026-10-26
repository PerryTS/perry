// Absent reserved holes use the same authoritative definition as dynamic keys.
function fresh(n: number) { const value = { first: n, next: null }; return value; }
const admitted = fresh(-1); admitted.reserved = 1; admitted.after = 2;
function report(label: string, o: any) {
  const d = Object.getOwnPropertyDescriptor(o, "reserved");
  console.log(label, Object.keys(o).join(","), Object.getOwnPropertyNames(o).join(","),
    d && d.value, d && d.writable, d && d.enumerable, d && d.configurable);
}
for (const key of ["reserved", "dynamic"]) {
  const o: any = fresh(1);
  Object.defineProperty(o, "after", { value: 9, enumerable: true });
  Object.defineProperty(o, key, { value: 7, enumerable: true });
  console.log("order", key, Object.keys(o).join(","), o[key]);
  o[key] = 11;
  console.log("sloppy", key, o[key]);
  function strictWrite(obj: any, k: string) { "use strict"; obj[k] = 12; }
  try { strictWrite(o, key); console.log("strict", "missed"); }
  catch (e) { console.log("strict", e instanceof TypeError); }
  console.log("reflect", Reflect.set(o, key, 13));
}
const omitted: any = fresh(2);
Object.defineProperty(omitted, "reserved", {});
report("omitted", omitted);
const data: any = fresh(3);
Object.defineProperty(data, "reserved", { value: 17, writable: true, configurable: true });
report("data", data);
Object.defineProperty(data, "reserved", { value: 23 });
report("redefine", data);
const shut: any = fresh(4);
Object.preventExtensions(shut);
console.log("closed", Reflect.defineProperty(shut, "reserved", { value: 1 }), "reserved" in shut);
let intercepted = 0;
Object.defineProperty(Object.prototype, "reserved", { set(v) { intercepted += v; }, configurable: true });
const inherited: any = fresh(5);
Object.defineProperty(inherited, "reserved", { value: 29 });
console.log("inherited", intercepted, inherited.reserved, Object.hasOwn(inherited, "reserved"));
delete Object.prototype.reserved;
const accessor: any = fresh(6);
Object.defineProperty(accessor, "reserved", { get() { return 31; }, configurable: true });
console.log("accessor", accessor.reserved, Object.getOwnPropertyDescriptor(accessor, "reserved").get !== undefined);
Object.defineProperty(accessor, "reserved", { value: 37 });
report("converted", accessor);

// The first name outside a family establishes its overflow, without a prior
// reserved-hole definition to make the receiver a dictionary first.
const firstOutside: any = fresh(7);
Object.defineProperty(firstOutside, "first_dynamic", { value: 41, enumerable: true });
const firstDescriptor = Object.getOwnPropertyDescriptor(firstOutside, "first_dynamic");
console.log("first-outside", Object.keys(firstOutside).join(","), firstOutside.first_dynamic,
  firstDescriptor.writable, firstDescriptor.enumerable, firstDescriptor.configurable,
  Reflect.set(firstOutside, "first_dynamic", 43));
const firstOmitted: any = fresh(8);
Object.defineProperty(firstOmitted, "first_omitted", {});
const omittedDescriptor = Object.getOwnPropertyDescriptor(firstOmitted, "first_omitted");
console.log("first-omitted", Object.getOwnPropertyNames(firstOmitted).join(","),
  omittedDescriptor.value, omittedDescriptor.writable, omittedDescriptor.enumerable,
  omittedDescriptor.configurable);
const firstGetter: any = fresh(9);
Object.defineProperty(firstGetter, "first_getter", { get() { return 47; } });
console.log("first-getter", firstGetter.first_getter,
  typeof Object.getOwnPropertyDescriptor(firstGetter, "first_getter").get);
