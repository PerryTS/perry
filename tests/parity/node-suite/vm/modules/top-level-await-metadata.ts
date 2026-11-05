// parity-node-argv: --experimental-vm-modules --no-warnings
// parity-env: PERRY_EXPERIMENTAL_VM_MODULES=1
import * as vm from "node:vm";

const sync = new vm.SourceTextModule("export const value = 1");
const asyncModule = new vm.SourceTextModule(
  "await Promise.resolve(); export const value = 2",
);

console.log(
  "before link:",
  sync.hasTopLevelAwait(),
  asyncModule.hasTopLevelAwait(),
);
await sync.link(() => {
  throw new Error("unexpected link");
});
await asyncModule.link(() => {
  throw new Error("unexpected link");
});
console.log(
  "after link:",
  sync.hasTopLevelAwait(),
  asyncModule.hasTopLevelAwait(),
);
