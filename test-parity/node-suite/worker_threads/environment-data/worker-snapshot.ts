import {
  getEnvironmentData,
  setEnvironmentData,
  Worker,
} from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/environment-data");

const original = { nested: { count: 1 }, values: [1, 2] };
setEnvironmentData("snapshot", original);

const worker = new Worker("./worker-snapshot-worker.cjs");
setEnvironmentData("snapshot", { nested: { count: 9 }, values: [9] });
original.nested.count = 7;

worker.on("message", (message) => {
  console.log("worker initial:", message.initialCount, message.values);
  console.log("worker local:", message.localCount);
  const current = getEnvironmentData("snapshot");
  console.log("parent current:", current?.nested?.count ?? "missing");
});
worker.on("exit", (code) => console.log("exit:", code));
