import * as vm from "node:vm";

const script = new vm.Script("1");
const context = vm.createContext();

function shape(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}

shape("this options", () => script.runInThisContext(null as any));
shape("context options", () => script.runInContext(context, "bad" as any));
shape("new options", () => script.runInNewContext({}, 1 as any));
shape("timeout type", () => script.runInThisContext({ timeout: "1" as any }));
shape("timeout range", () => script.runInThisContext({ timeout: 0 }));
shape(
  "displayErrors",
  () => script.runInThisContext({ displayErrors: 1 as any }),
);
shape(
  "breakOnSigint",
  () => script.runInThisContext({ breakOnSigint: null as any }),
);
