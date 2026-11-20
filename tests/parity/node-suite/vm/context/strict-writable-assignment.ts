import * as vm from "node:vm";

const sandbox: any = { value: 2 };
const context = vm.createContext(sandbox);

console.log(
  "strict existing:",
  vm.runInContext("'use strict'; value = value + 3; value", context),
  sandbox.value,
);
console.log(
  "strict explicit global:",
  vm.runInContext("'use strict'; globalThis.added = 7; added", context),
  sandbox.added,
);
