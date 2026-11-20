import * as vm from "node:vm";

const context = vm.createContext({});
const descriptor: any = vm.runInContext(
  "Object.getOwnPropertyDescriptor({ value: 1 }, 'value')",
  context,
);

console.log(
  "flags:",
  descriptor?.enumerable,
  descriptor?.configurable,
  descriptor?.writable,
);
console.log("value:", typeof descriptor?.value, descriptor?.value);
console.log(
  "prototype:",
  descriptor === undefined
    ? false
    : Object.getPrototypeOf(descriptor) === Object.prototype,
);
