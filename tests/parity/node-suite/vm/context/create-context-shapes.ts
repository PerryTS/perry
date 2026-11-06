import * as vm from "node:vm";

const omitted: any = vm.createContext();
const object: any = { value: 1 };

console.log("omitted:", typeof omitted, vm.isContext(omitted));
console.log(
  "object identity:",
  vm.createContext(object) === object,
  vm.isContext(object),
);
console.log(
  "repeat identity:",
  vm.createContext(object) === object,
  vm.isContext(object),
);
console.log("plain markers:", vm.isContext({}), vm.isContext([]));
