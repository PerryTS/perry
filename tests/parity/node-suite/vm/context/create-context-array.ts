import * as vm from "node:vm";

const array: any[] = [1, 2];
console.log("identity:", vm.createContext(array) === array);
console.log("marker:", vm.isContext(array));
console.log("length:", array.length);
console.log(
  "execution:",
  vm.runInContext("length = length + 1; length", array),
  array.length,
);
