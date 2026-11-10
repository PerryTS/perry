import { Worker } from "node:worker_threads";

process.chdir("tests/parity/node-suite/worker_threads/worker-lifecycle");
const worker = new Worker("./thread-metadata-worker.cjs", {
  name: "direct-metadata",
  resourceLimits: { stackSizeMb: 2 },
});
console.log("live:", worker.threadId > 0, worker.threadName,
  worker.resourceLimits?.stackSizeMb);
const originalId = worker.threadId;
const limits = worker.resourceLimits;
console.log("snapshot identity:", limits === worker.resourceLimits);
if (limits !== undefined) {
  limits.stackSizeMb = 77;
}
console.log("snapshot isolated:", worker.resourceLimits?.stackSizeMb);
const wrappers = [["derived", Object.create(worker)], ["proxy", new Proxy(worker, {})]];
function wrappedMetadata(phase: string) {
  for (const [label, receiver] of wrappers) {
    try {
      const first = receiver.resourceLimits;
      const second = receiver.resourceLimits;
      first.stackSizeMb = 77;
      console.log("wrapped:", phase, label, receiver.threadId > 0, receiver.threadId === -1,
        receiver.threadName, second.stackSizeMb, first === second);
    } catch (error) {
      console.log("wrapped:", phase, label, error.name);
    }
  }
}
wrappedMetadata("live");
worker.on("online", () => {
  console.log("online:", worker.threadId > 0, worker.threadId === originalId,
    worker.threadName, worker.resourceLimits?.stackSizeMb);
});
worker.on("exit", () => {
  wrappedMetadata("closed");
  console.log("closed:", worker.threadId, worker.threadName,
    JSON.stringify(worker.resourceLimits), worker.resourceLimits === worker.resourceLimits);
});
