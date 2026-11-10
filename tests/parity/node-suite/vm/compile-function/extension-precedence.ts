import * as vm from "node:vm";

const context = vm.createContext({ fallback: "context" });
const first: any = { left: "left", shared: "first" };
const second: any = { right: "right", shared: "second" };
const fn: any = vm.compileFunction(
  "return left + ':' + shared + ':' + right + ':' + fallback",
  [],
  { parsingContext: context, contextExtensions: [first, second] },
);

console.log("precedence:", fn());
first.shared = "changed-first";
second.shared = "changed-second";
console.log("live precedence:", fn());
