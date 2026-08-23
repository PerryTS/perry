const descriptor = Object.getOwnPropertyDescriptor(Intl.Collator.prototype, "compare")!;
const getter = descriptor.get!;
const collator = new Intl.Collator("en");
const compare = collator.compare;

console.log(
  typeof getter,
  descriptor.set,
  descriptor.enumerable,
  descriptor.configurable,
);
console.log(
  getter.name,
  getter.length,
  Object.prototype.hasOwnProperty.call(getter, "prototype"),
);
console.log(
  compare === collator.compare,
  getter.call(collator) === compare,
  compare.name,
  compare.length,
  Object.prototype.hasOwnProperty.call(compare, "prototype"),
);

try {
  getter.call({});
} catch (error) {
  console.log(error instanceof TypeError);
}
