import * as vm from "node:vm";

const base: any = { inherited: 2 };
const sandbox: any = Object.create(base);
sandbox.own = 3;
const context = vm.createContext(sandbox);

console.log("read:", vm.runInContext("inherited + own", context));
console.log("write:", vm.runInContext("inherited = 5; inherited", context));
console.log(
  "ownership:",
  Object.prototype.hasOwnProperty.call(sandbox, "inherited"),
  sandbox.inherited,
  base.inherited,
);
