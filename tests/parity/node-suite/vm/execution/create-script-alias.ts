import * as vm from "node:vm";

const direct = new vm.Script("value = value + 1; value");
const factory = vm.createScript("value = value + 1; value");
const one: any = vm.createContext({ value: 1 });
const two: any = vm.createContext({ value: 10 });

console.log(
  "instances:",
  direct instanceof vm.Script,
  factory instanceof vm.Script,
);
console.log(
  "constructor:",
  direct.constructor === vm.Script,
  factory.constructor === vm.Script,
);
console.log("direct:", direct.runInContext(one), one.value);
console.log("factory:", factory.runInContext(two), two.value);
