// node:vm CommonJS require surface.
const vm = require("node:vm");
const vmBare = require("vm");

console.log("node prefix identity:", vm === vmBare);
console.log("require keys:", Object.keys(vm).join(","));

for (
  const name of [
    "Script",
    "createContext",
    "createScript",
    "runInContext",
    "runInNewContext",
    "runInThisContext",
    "isContext",
    "compileFunction",
    "measureMemory",
  ] as const
) {
  const value = vm[name];
  console.log(
    name,
    typeof value,
    typeof value === "function" ? value.length : "-",
  );
}

const builtin = process.getBuiltinModule("vm");
console.log("builtin identity:", builtin === vm);
console.log("builtin node prefix:", process.getBuiltinModule("node:vm") === vm);
console.log("constants keys:", Object.keys(vm.constants).join(","));
console.log(
  "constants symbols:",
  typeof vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER,
  String(vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER),
  typeof vm.constants.DONT_CONTEXTIFY,
  String(vm.constants.DONT_CONTEXTIFY),
);
console.log("isContext plain:", vm.isContext({}));

for (const name of ["Module", "SourceTextModule", "SyntheticModule"] as const) {
  console.log(
    "gated",
    name,
    Object.prototype.hasOwnProperty.call(vm, name),
    typeof vm[name],
  );
}
