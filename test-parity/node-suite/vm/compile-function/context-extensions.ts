import * as vm from "node:vm";

const context = vm.createContext({ shared: 10 });
const extension = { shared: 20, extra: 3 };
const fn: any = vm.compileFunction("return shared + extra + arg", ["arg"], {
  parsingContext: context,
  contextExtensions: [extension],
});

console.log("result:", fn(4));
extension.extra = 5;
console.log("live extension:", fn(4));
console.log("length name:", fn.length, typeof fn.name, fn.name);
