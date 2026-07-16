import * as vm from "node:vm";

const symbol = Symbol("symbol");
const sandbox: any = { visible: 1, symbol, [symbol]: 3 };
Object.defineProperty(sandbox, "hidden", { value: 2 });
const context = vm.createContext(sandbox);

console.log(
  "keys:",
  vm.runInContext("Object.keys(globalThis).includes('visible')", context),
  vm.runInContext("Object.keys(globalThis).includes('hidden')", context),
);
console.log(
  "names:",
  vm.runInContext(
    "Object.getOwnPropertyNames(globalThis).includes('visible')",
    context,
  ),
  vm.runInContext(
    "Object.getOwnPropertyNames(globalThis).includes('hidden')",
    context,
  ),
);
console.log(
  "symbols:",
  vm.runInContext(
    "Object.getOwnPropertySymbols(globalThis).includes(symbol)",
    context,
  ),
);
