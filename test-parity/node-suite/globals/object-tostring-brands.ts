function ordinary() {}
const arrow = () => {};
class Klass {}

for (const [label, value] of [
  ["ordinary", ordinary],
  ["arrow", arrow],
  ["class", Klass],
  ["Array", Array],
  ["Math.max", Math.max],
  ["Math", Math],
  ["JSON", JSON],
  ["Reflect", Reflect],
] as const) {
  console.log(label + ":", Object.prototype.toString.call(value));
}

for (const [label, value] of [
  ["Math", Math],
  ["JSON", JSON],
  ["Reflect", Reflect],
] as const) {
  const desc = Object.getOwnPropertyDescriptor(value, Symbol.toStringTag);
  const rendered = desc
    ? JSON.stringify([desc.value, desc.writable, desc.enumerable, desc.configurable])
    : "missing";
  console.log(label + " tag desc:", rendered);
}

ordinary[Symbol.toStringTag] = "CallableOverride";
console.log("function override:", Object.prototype.toString.call(ordinary));

const namespaceOverride = Object.create(Math);
Object.defineProperty(namespaceOverride, Symbol.toStringTag, { value: "NamespaceOverride" });
console.log("namespace override:", Object.prototype.toString.call(namespaceOverride));
