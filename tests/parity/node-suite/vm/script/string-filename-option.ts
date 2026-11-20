import * as vm from "node:vm";

function containsFilename(value: unknown, filename: string) {
  return String(value).includes(filename);
}

console.log(
  "Script:",
  containsFilename(
    new vm.Script("new Error().stack", "script-string.vm").runInThisContext(),
    "script-string.vm",
  ),
);
console.log(
  "runThis:",
  containsFilename(
    vm.runInThisContext("new Error().stack", "this-string.vm"),
    "this-string.vm",
  ),
);
console.log(
  "runNew:",
  containsFilename(
    vm.runInNewContext("new Error().stack", {}, "new-string.vm"),
    "new-string.vm",
  ),
);
const context = vm.createContext({});
console.log(
  "runContext:",
  containsFilename(
    vm.runInContext("new Error().stack", context, "context-string.vm"),
    "context-string.vm",
  ),
);
