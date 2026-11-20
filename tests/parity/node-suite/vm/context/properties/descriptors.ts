import * as vm from "node:vm";

const sandbox: any = {};
Object.defineProperty(sandbox, "fixed", {
  value: 7,
  writable: false,
  enumerable: false,
  configurable: false,
});
const context = vm.createContext(sandbox);
const descriptor: any = vm.runInContext(
  "Object.getOwnPropertyDescriptor(globalThis, 'fixed')",
  context,
);

console.log("value:", descriptor?.value);
console.log(
  "flags:",
  descriptor?.writable,
  descriptor?.enumerable,
  descriptor?.configurable,
);
console.log("outer keys:", Object.keys(sandbox).includes("fixed"));
