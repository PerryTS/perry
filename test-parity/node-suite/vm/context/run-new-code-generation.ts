import * as vm from "node:vm";

try {
  vm.runInNewContext("eval('1')", {}, {
    contextCodeGeneration: { strings: false, wasm: true },
  });
  console.log("disabled: ok");
} catch (error: any) {
  console.log("disabled:", error.name, error.code || "-");
}
console.log(
  "enabled:",
  vm.runInNewContext("eval('2 + 3')", {}, {
    contextCodeGeneration: { strings: true },
  }),
);
try {
  vm.runInNewContext("1", {}, { contextCodeGeneration: null as any });
  console.log("validation: ok");
} catch (error: any) {
  console.log("validation:", error.name, error.code || "-");
}
