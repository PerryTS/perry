import * as vm from "node:vm";

const sandbox: any = { value: 1 };
const context = vm.createContext(sandbox);
const contextGlobal: any = vm.runInContext("this", context);

console.log("identity:", contextGlobal === sandbox);
contextGlobal.value = 4;
contextGlobal.added = 5;
console.log("outer write:", sandbox.value, sandbox.added);
sandbox.value = 6;
console.log(
  "inner read:",
  contextGlobal.value,
  vm.runInContext("value", context),
);
console.log("own:", Object.hasOwn(contextGlobal, "added"));
