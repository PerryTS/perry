import vm, * as namespace from "node:vm";

const expected = [
  "Script",
  "compileFunction",
  "constants",
  "createContext",
  "createScript",
  "isContext",
  "measureMemory",
  "runInContext",
  "runInNewContext",
  "runInThisContext",
];

console.log(
  "exports:",
  expected.map((key) => key + ":" + typeof (vm as any)[key]).join(","),
);
console.log("namespace default:", namespace.default === vm);
console.log(
  "script class:",
  typeof vm.Script,
  vm.Script.name,
  vm.Script.length,
);
console.log("constants frozen:", Object.isFrozen(vm.constants));
console.log(
  "constants null prototype:",
  Object.getPrototypeOf(vm.constants) === null,
);
