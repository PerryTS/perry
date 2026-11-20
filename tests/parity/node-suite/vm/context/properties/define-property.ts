import * as vm from "node:vm";

const sandbox: any = {};
const context = vm.createContext(sandbox);
vm.runInContext(
  "Object.defineProperty(globalThis, 'named', { value: 8, writable: false, enumerable: true, configurable: true })",
  context,
);
vm.runInContext(
  "Object.defineProperty(globalThis, 99, { value: 20, enumerable: true })",
  context,
);

const named = Object.getOwnPropertyDescriptor(sandbox, "named");
const indexed = Object.getOwnPropertyDescriptor(sandbox, "99");
console.log("values:", sandbox.named, sandbox[99]);
console.log(
  "named flags:",
  named?.writable,
  named?.enumerable,
  named?.configurable,
);
console.log(
  "indexed flags:",
  indexed?.writable,
  indexed?.enumerable,
  indexed?.configurable,
);
