import { resourceLimits, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const requested = {
  maxOldGenerationSizeMb: 16,
  maxYoungGenerationSizeMb: 4,
  codeRangeSizeMb: 16,
  stackSizeMb: 1,
};

console.log("main:", JSON.stringify(resourceLimits));
const worker = new Worker("./resource-limits-metadata-worker.cjs", {
  resourceLimits: requested,
});
console.log("parent live:", JSON.stringify(worker.resourceLimits));
worker.on("message", (value) => {
  console.log("worker:", JSON.stringify(value));
});
worker.on("error", (error: any) => {
  console.log("error:", error?.name, error?.code ?? "");
});
worker.on("exit", (code) => {
  console.log("exit:", code, JSON.stringify(worker.resourceLimits));
});
