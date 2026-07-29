import { Recoverable } from "node:repl";

console.log(Object.getPrototypeOf(Recoverable) === SyntaxError);
console.log(
  Object.getPrototypeOf(Recoverable.prototype) === SyntaxError.prototype,
);
const descriptor = Object.getOwnPropertyDescriptor(
  Recoverable.prototype,
  "constructor",
)!;
console.log(
  descriptor.value === Recoverable,
  descriptor.enumerable,
  descriptor.configurable,
  descriptor.writable,
);
