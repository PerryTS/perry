// node:vm Script execution and object-backed context semantics.
import vm from "node:vm";

function errorShape(label: string, fn: () => void) {
  try {
    fn();
    console.log(label + ":", "ok");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}

const sandbox: any = { x: 1 };
const context = vm.createContext(sandbox);

console.log("context identity:", context === sandbox);
console.log("context markers:", vm.isContext(context), vm.isContext({}));
errorShape("createContext validation", () => vm.createContext(123 as any));

const script = new vm.Script("x = x + 2; y = x + 1; y");
console.log(
  "context run:",
  script.runInContext(context),
  sandbox.x,
  sandbox.y,
  typeof (globalThis as any).y,
);
errorShape(
  "run plain object validation",
  () => vm.runInContext("1", {} as any),
);

try {
  (vm as any).Script("1");
  console.log("script call validation:", "ok");
} catch (error: any) {
  console.log("script call validation:", error.name, error.code || "-");
}

(globalThis as any).vmCounter = 0;
const repeat = new vm.Script("vmCounter = vmCounter + 1; vmCounter");
console.log(
  "this repeat:",
  repeat.runInThisContext(),
  repeat.runInThisContext(),
  (globalThis as any).vmCounter,
);

const fresh: any = { x: 9 };
console.log(
  "new context:",
  vm.runInNewContext("x = x + 1; result = x; result", fresh),
  fresh.x,
  typeof (globalThis as any).result,
);

console.log(
  "runInContext:",
  vm.runInContext("x = x + 1; x", context),
  sandbox.x,
);
console.log(
  "createScript:",
  vm.createScript("x = x + 1; x").runInContext(context),
  sandbox.x,
);
