// parity-node-argv: --experimental-vm-modules --no-warnings
// parity-env: PERRY_EXPERIMENTAL_VM_MODULES=1
import * as vm from "node:vm";

const module = new vm.SourceTextModule("throw new TypeError('boom')", {
  identifier: "error.vm",
});
await module.link(() => {
  throw new Error("unexpected link");
});
let caught: any;
try {
  await module.evaluate();
  console.log("evaluate: ok");
} catch (error: any) {
  caught = error;
  console.log("evaluate:", error.name, error.code || "-");
}
console.log("status:", module.status);
try {
  console.log(
    "stored error:",
    module.error.name,
    module.error.code || "-",
    module.error === caught,
  );
} catch (error: any) {
  console.log("stored error:", error.name, error.code || "-");
}
