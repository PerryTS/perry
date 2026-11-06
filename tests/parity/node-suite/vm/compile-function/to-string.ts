import * as vm from "node:vm";

const fn: any = vm.compileFunction("return left + right", ["left", "right"]);
const source = Function.prototype.toString.call(fn);

console.log("shape:", source.startsWith("function (left, right)"));
console.log("body:", source.includes("return left + right"));
console.log("metadata:", fn.name, fn.length, Object.hasOwn(fn, "prototype"));
