import childProcessDefault from "node:child_process";
import * as childProcessNs from "node:child_process";

const key = "_forkChild";
const nsHelper = (childProcessNs as any)._forkChild;
const defaultHelper = (childProcessDefault as any)._forkChild;

console.log("namespace keys include _forkChild:", Object.keys(childProcessNs).includes(key));
console.log(
  "namespace enumerable _forkChild:",
  Object.prototype.propertyIsEnumerable.call(childProcessNs, key),
);
console.log("namespace _forkChild type:", typeof nsHelper);
console.log("namespace _forkChild length:", nsHelper?.length);
console.log("namespace _forkChild name:", nsHelper?.name);
console.log("default keys include _forkChild:", Object.keys(childProcessDefault).includes(key));
console.log(
  "default enumerable _forkChild:",
  Object.prototype.propertyIsEnumerable.call(childProcessDefault, key),
);
console.log("default _forkChild type:", typeof defaultHelper);
console.log("default _forkChild length:", defaultHelper?.length);
console.log("default _forkChild name:", defaultHelper?.name);
console.log("namespace/default identity:", nsHelper === defaultHelper);
