import * as vm from "node:vm";

function shape(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}

shape("Script code", () => new vm.Script(1 as any));
shape("Script options", () => new vm.Script("1", 1 as any));
shape("createScript code", () => vm.createScript(null as any));
shape("runThis code", () => vm.runInThisContext({} as any));
shape("runNew code", () => vm.runInNewContext(undefined as any));
shape("runContext code", () => vm.runInContext(1 as any, vm.createContext()));
shape("runContext plain", () => vm.runInContext("1", {} as any));
