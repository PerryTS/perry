// parity-node-argv: --experimental-vm-modules --no-warnings
// parity-env: PERRY_EXPERIMENTAL_VM_MODULES=1
import * as vm from "node:vm";

const context: any = vm.createContext({ evaluations: 0 });
const module = new vm.SourceTextModule(
  "evaluations = evaluations + 1; export const value = evaluations",
  { context, identifier: "repeat.vm" },
);

await module.link(() => {
  throw new Error("unexpected link");
});
const first = module.evaluate();
console.log("first promise:", typeof first?.then);
await first;
console.log(
  "first:",
  module.status,
  module.namespace.value,
  context.evaluations,
);
await module.evaluate();
console.log(
  "second:",
  module.status,
  module.namespace.value,
  context.evaluations,
);
