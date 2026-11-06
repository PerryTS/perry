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

shape("source call", () => (vm.SourceTextModule as any)(""));
shape("source code", () => new vm.SourceTextModule(1 as any));
shape("source options", () => new vm.SourceTextModule("", null as any));
shape(
  "source context",
  () => new vm.SourceTextModule("", { context: {} as any }),
);
shape(
  "identifier",
  () => new vm.SourceTextModule("", { identifier: 1 as any }),
);
shape("synthetic call", () => (vm.SyntheticModule as any)([], () => {}));
shape(
  "synthetic exports",
  () => new vm.SyntheticModule("value" as any, () => {}),
);
shape("synthetic callback", () => new vm.SyntheticModule([], null as any));
shape(
  "synthetic context",
  () => new vm.SyntheticModule([], () => {}, { context: {} as any }),
);
