import { mock } from "node:test";

function original(a: unknown, b: unknown) {
  return [a, b];
}

const fn = mock.fn(original);
console.log("name:", original.name, fn.name);
console.log("length:", original.length, fn.length);
console.log(
  "descriptors:",
  Object.getOwnPropertyDescriptor(fn, "name")?.configurable,
  Object.getOwnPropertyDescriptor(fn, "length")?.configurable,
);
mock.restoreAll();
