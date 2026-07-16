import { Module } from "node:module";

console.log(
  "constructor:",
  Module.name,
  Module.length,
  Module.Module === Module,
);
console.log(
  "prototype names:",
  Object.getOwnPropertyNames(Module.prototype).sort().join(","),
);
console.log(
  "prototype chain:",
  Object.getPrototypeOf(Module.prototype) === Object.prototype,
);

const parent = new Module("parent-id");
const child = new Module("child-id", parent);
console.log(
  "parent defaults:",
  parent.id,
  JSON.stringify(parent.path),
  JSON.stringify(parent.filename),
  parent.loaded,
  parent.children.length,
  Array.isArray(parent.paths),
);
console.log(
  "child relationship:",
  child.parent === parent,
  parent.children[0] === child,
  parent.children.length,
);
console.log(
  "child exports prototype:",
  Object.getPrototypeOf(child.exports) === Object.prototype,
);
console.log(
  "instanceof:",
  child instanceof Module,
  child.constructor === Module,
);
