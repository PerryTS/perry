import * as vm from "node:vm";

const fn: any = vm.compileFunction(
  "return arguments.length + ':' + a + ':' + b + ':' + this.marker",
  ["a", "b"],
);

console.log("length:", fn.length);
console.log("missing:", fn.call({ marker: "m" }, 1));
console.log("extra:", fn.call({ marker: "e" }, 1, 2, 3));
