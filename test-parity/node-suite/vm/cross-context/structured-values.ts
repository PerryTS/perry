import * as vm from "node:vm";

const context = vm.createContext({});
const object: any = vm.runInContext("({ value: 1 })", context);
const array: any = vm.runInContext("[1, 2, 3]", context);
const error: any = vm.runInContext("new TypeError('boom')", context);

console.log(
  "object:",
  object.value,
  Object.getPrototypeOf(object) === Object.prototype,
);
console.log(
  "array:",
  Array.isArray(array),
  array.length,
  Object.getPrototypeOf(array) === Array.prototype,
);
console.log(
  "error:",
  typeof error,
  error?.name,
  error instanceof TypeError,
);
console.log(
  "context brands:",
  vm.runInContext("Object.prototype.toString.call([])", context),
);
