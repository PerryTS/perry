import * as vm from "node:vm";

const context = vm.createContext({});
const promise: any = vm.runInContext("Promise.resolve({ value: 7 })", context);
console.log("thenable:", typeof promise?.then, promise instanceof Promise);
const value = await promise;
console.log(
  "resolved:",
  value?.value,
  value === undefined
    ? false
    : Object.getPrototypeOf(value) === Object.prototype,
);
