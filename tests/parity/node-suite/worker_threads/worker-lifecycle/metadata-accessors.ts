import { Worker } from "node:worker_threads";

process.chdir("tests/parity/node-suite/worker_threads/worker-lifecycle");
const worker = new Worker("./thread-metadata-worker.cjs", {
  name: "metadata",
  resourceLimits: { stackSizeMb: 2 },
});
console.log("live before descriptors:", worker.threadId > 0, worker.threadName,
  worker.resourceLimits?.stackSizeMb);
const prototype = Object.getPrototypeOf(worker);
console.log("prototype:", prototype === Worker.prototype, worker.constructor === Worker);
for (const key of ["threadId", "threadName", "resourceLimits"]) {
  const descriptor = Object.getOwnPropertyDescriptor(prototype, key);
  console.log("descriptor:", key, Object.hasOwn(worker, key),
    descriptor !== undefined, typeof descriptor?.get, descriptor?.set === undefined,
    descriptor?.enumerable, descriptor?.configurable);
  try {
    worker[key] = 42;
    console.log("assignment:", key, "accepted");
  } catch (error) {
    console.log("assignment:", key, error.name, error.code === undefined);
  }
}
const receivers = [["derived", Object.create(worker)], ["proxy", new Proxy(worker, {})]];
function borrowedMetadata(phase: string) {
  for (const key of ["threadId", "threadName", "resourceLimits"]) {
    const getter = Object.getOwnPropertyDescriptor(Worker.prototype, key).get;
    for (const [label, receiver] of receivers) {
      try {
        const value = getter.call(receiver);
        console.log("borrowed:", phase, key, label,
          key === "threadId" ? value > 0 : key === "resourceLimits" ? value.stackSizeMb : value);
      } catch (error) {
        console.log("borrowed:", phase, key, label, error.name);
      }
    }
  }
}
borrowedMetadata("live");
const limits = worker.resourceLimits;
limits.stackSizeMb = 77;
console.log("limits snapshot:", limits === worker.resourceLimits, worker.resourceLimits.stackSizeMb);
const originalId = worker.threadId;
Object.defineProperty(worker, "threadId", { value: originalId, configurable: true });
Object.defineProperty(worker, "threadName", { value: "shadow", configurable: true });
Object.defineProperty(worker, "resourceLimits", { value: limits, configurable: true });
worker.on("exit", () => {
  console.log("shadows:", worker.threadId === originalId, worker.threadName,
    worker.resourceLimits === limits);
  delete worker.threadId;
  delete worker.threadName;
  delete worker.resourceLimits;
  borrowedMetadata("closed");
  console.log("closed:", worker.threadId, worker.threadName,
    JSON.stringify(worker.resourceLimits), worker.resourceLimits === worker.resourceLimits);
});
