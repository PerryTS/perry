// parity-node-argv: --experimental-vm-modules --no-warnings
// parity-env: PERRY_EXPERIMENTAL_VM_MODULES=1
import * as vm from "node:vm";

function shape(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}

shape(
  "export",
  () =>
    new vm.SourceTextModule("export const = 1", { identifier: "export.vm" }),
);
shape(
  "import",
  () =>
    new vm.SourceTextModule("import { value from 'dep'", {
      identifier: "import.vm",
    }),
);
