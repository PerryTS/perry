import * as vm from "node:vm";

const marker = Symbol("marker");
const hidden = Symbol("hidden");
const sandbox: any = { marker, hidden, [marker]: "visible" };
Object.defineProperty(sandbox, hidden, { value: "hidden" });
const context = vm.createContext(sandbox);

console.log(
  "forwarded symbols:",
  vm.runInContext(
    "Object.getOwnPropertySymbols(globalThis).includes(marker)",
    context,
  ),
  vm.runInContext(
    "Object.getOwnPropertySymbols(globalThis).includes(hidden)",
    context,
  ),
);
console.log(
  "values:",
  vm.runInContext("globalThis[marker]", context),
  vm.runInContext("globalThis[hidden]", context),
);
console.log(
  "hidden enumerable:",
  vm.runInContext(
    "Object.getOwnPropertyDescriptor(globalThis, hidden).enumerable",
    context,
  ),
);
