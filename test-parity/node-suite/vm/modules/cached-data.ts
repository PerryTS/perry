// parity-node-argv: --experimental-vm-modules --no-warnings
// parity-env: PERRY_EXPERIMENTAL_VM_MODULES=1
// node:vm SourceTextModule cached-data observable shape.
import * as vm from "node:vm";

function errorShape(label: string, fn: () => void) {
  try {
    fn();
    console.log(label + ":", "ok");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}

const source = "export const value = 42;";
const module = new vm.SourceTextModule(source, { identifier: "cache-fixture" });
const cachedData = module.createCachedData();

console.log(
  "module cache shape:",
  Buffer.isBuffer(cachedData),
  cachedData.length > 0,
  module.status,
);

const accepted = new vm.SourceTextModule(source, { cachedData });
console.log("module cache accepted:", accepted.status);

errorShape("module cachedData validation", () => {
  new vm.SourceTextModule(source, { cachedData: "nope" as any });
});

errorShape("module cachedData rejected", () => {
  new vm.SourceTextModule(source, { cachedData: Buffer.from([0]) });
});

await module.link(() => {});
await module.evaluate();

errorShape("module evaluated cache", () => {
  module.createCachedData();
});
