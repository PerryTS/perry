// parity-node-argv: --experimental-vm-modules --no-warnings
// parity-env: PERRY_EXPERIMENTAL_VM_MODULES=1
import * as vm from "node:vm";

const context: any = vm.createContext({ seed: 4 });
const module = new vm.SourceTextModule(
  "export const answer = seed + 1; globalThis.created = answer + 1",
  { context, identifier: "context-module" },
);

console.log("initial:", module.status, module.identifier);
try {
  console.log("initial namespace:", module.namespace.answer);
} catch (error: any) {
  console.log("initial namespace:", error.name, error.code || "-");
}
await module.link(() => {
  throw new Error("unexpected link");
});
console.log("linked:", module.status);
await module.evaluate();
console.log(
  "evaluated:",
  module.status,
  module.namespace.answer,
  context.created,
);
console.log("namespace tag:", Object.prototype.toString.call(module.namespace));
console.log(
  "namespace prototype:",
  Object.getPrototypeOf(module.namespace) === null,
);
