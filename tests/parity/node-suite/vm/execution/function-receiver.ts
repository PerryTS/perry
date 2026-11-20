import * as vm from "node:vm";

const main: any = vm.compileFunction("return this");
const context: any = vm.createContext({ marker: 7 });
const bound: any = vm.compileFunction("return this", [], {
  parsingContext: context,
});

console.log("main undefined call:", main() === globalThis);
console.log("main receiver:", main.call({ marker: 1 }).marker);
console.log(
  "bound undefined call:",
  bound() === vm.runInContext("globalThis", context),
);
console.log("bound receiver:", bound.call({ marker: 2 }).marker);
