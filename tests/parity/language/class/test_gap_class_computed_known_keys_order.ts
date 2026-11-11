// THR1: compile-time-known computed keys are ordinary prototype keys, in
// OwnPropertyKeys order (integer keys ascending, then strings in insertion
// order, then symbols in insertion order), with method attributes.
class C {
  b() { return "b"; }
  ["0"]() { return "zero"; }
  *[Symbol.iterator]() { yield 1; yield 2; }
  a() { return "a"; }
  [2]() { return "two"; }
  ["c"]() { return "c"; }
  [Symbol.toPrimitive]() { return 7; }
  get ["g"]() { return "g"; }
  ["1e3"]() { return "str1e3"; }
  [1e3]() { return "num1000"; }
  ["b"]() { return "b2"; }
}
const keys = Reflect.ownKeys(C.prototype).map(String);
console.log(keys.join(","));
const c = new C();
console.log(c.b(), c["0"](), c[0](), c.a(), c[2](), c.c(), c.g, c["1e3"](), c[1000]());
console.log([...c].join("|"), +c);
for (const k of Reflect.ownKeys(C.prototype)) {
  const d = Object.getOwnPropertyDescriptor(C.prototype, k);
  console.log(String(k), d.enumerable, d.configurable, "value" in d ? d.writable : "acc");
}
console.log(Object.keys(C.prototype).length, JSON.stringify(Object.getOwnPropertyNames(C.prototype)));
