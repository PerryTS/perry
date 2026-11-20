import "node:domain";

const descriptor = Object.getOwnPropertyDescriptor(process, "domain")!;
console.log(descriptor.enumerable, descriptor.configurable);
console.log(
  typeof descriptor.get,
  typeof descriptor.set,
  "value" in descriptor,
);
console.log(String((process as any).domain));
