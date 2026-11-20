import domain from "node:domain";

const d = domain.create();
const descriptor = Object.getOwnPropertyDescriptor(d, "members")!;
console.log(
  descriptor.enumerable,
  descriptor.configurable,
  descriptor.writable,
);
console.log(descriptor.value === d.members);
