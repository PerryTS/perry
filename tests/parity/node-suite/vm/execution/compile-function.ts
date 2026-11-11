// node:vm compileFunction context binding and validation.
import * as vm from "node:vm";

function errorShape(label: string, fn: () => void) {
  try {
    fn();
    console.log(label + ":", "ok");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}

const sandbox: any = { z: 5 };
const context = vm.createContext(sandbox);
const fn: any = vm.compileFunction("return a + b + globalThis.z", ["a", "b"], {
  parsingContext: context,
});

console.log(
  "compile bound:",
  fn.length,
  fn(3, 4),
  typeof (globalThis as any).z,
);

const mainFn: any = vm.compileFunction("return a + b", ["a", "b"]);
console.log("compile main:", mainFn.length, mainFn(10, 20));

errorShape("compile code validation", () => vm.compileFunction(1 as any));
errorShape(
  "compile params validation",
  () => vm.compileFunction("return 1", "x" as any),
);
errorShape(
  "compile context validation",
  () => vm.compileFunction("return 1", [], { parsingContext: {} as any }),
);

console.log(
  "constants:",
  Object.keys(vm.constants).join(","),
  String(vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER),
  String(vm.constants.DONT_CONTEXTIFY),
);
