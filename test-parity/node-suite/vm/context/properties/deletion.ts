import * as vm from "node:vm";

const sandbox: any = { removable: 3, retained: 4 };
const context = vm.createContext(sandbox);

console.log(
  "delete result:",
  vm.runInContext("delete globalThis.removable", context),
);
console.log("outer:", "removable" in sandbox, sandbox.retained);
console.log("context:", vm.runInContext("typeof removable", context));
