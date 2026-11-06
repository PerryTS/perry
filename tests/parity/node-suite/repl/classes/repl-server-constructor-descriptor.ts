import { REPLServer } from "node:repl";

const descriptor = Object.getOwnPropertyDescriptor(
  REPLServer.prototype,
  "constructor",
)!;
console.log(descriptor.value === REPLServer);
console.log(
  descriptor.enumerable,
  descriptor.configurable,
  descriptor.writable,
);
