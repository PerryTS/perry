import * as vm from "node:vm";

const context = vm.createContext({});
const names = [
  "Object",
  "Array",
  "Error",
  "TypeError",
  "Promise",
  "Map",
  "Set",
];
for (const name of names) {
  const value = vm.runInContext(name, context);
  console.log(name + ":", typeof value, value === (globalThis as any)[name]);
}
console.log(
  "prototype identity:",
  vm.runInContext("Object.prototype", context) === Object.prototype,
  vm.runInContext("Array.prototype", context) === Array.prototype,
);
