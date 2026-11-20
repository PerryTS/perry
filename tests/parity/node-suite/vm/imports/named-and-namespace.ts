// node:vm no-flag ESM import surface.
import vmDefault, * as vm from "node:vm";
import {
  compileFunction,
  constants,
  createContext,
  createScript,
  isContext,
  measureMemory,
  runInContext,
  runInNewContext,
  runInThisContext,
  Script,
} from "node:vm";

for (
  const [name, value] of [
    ["Script", Script],
    ["createContext", createContext],
    ["createScript", createScript],
    ["runInContext", runInContext],
    ["runInNewContext", runInNewContext],
    ["runInThisContext", runInThisContext],
    ["isContext", isContext],
    ["compileFunction", compileFunction],
    ["measureMemory", measureMemory],
  ] as const
) {
  console.log(
    name,
    typeof value,
    typeof value === "function" ? (value as Function).length : "-",
    typeof (vm as any)[name],
    (vm as any)[name] === value,
  );
}

console.log("default:", typeof vmDefault, vmDefault === vm.default);
console.log("default Script:", (vmDefault as any).Script === Script);
console.log("constants identity:", constants === vm.constants);
console.log("constants keys:", Object.keys(constants).join(","));
console.log(
  "constants symbols:",
  typeof constants.USE_MAIN_CONTEXT_DEFAULT_LOADER,
  String(constants.USE_MAIN_CONTEXT_DEFAULT_LOADER),
  typeof constants.DONT_CONTEXTIFY,
  String(constants.DONT_CONTEXTIFY),
);
console.log("isContext plain:", isContext({}));

for (const name of ["Module", "SourceTextModule", "SyntheticModule"] as const) {
  console.log(
    "gated",
    name,
    Object.prototype.hasOwnProperty.call(vm, name),
    typeof (vm as any)[name],
  );
}
