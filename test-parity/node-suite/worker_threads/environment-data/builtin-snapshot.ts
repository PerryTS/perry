import {
  getEnvironmentData,
  setEnvironmentData,
  Worker,
} from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/environment-data");

const map = new Map<string, any>([
  ["date", new Date("2022-03-04T05:06:07.000Z")],
  ["set", new Set([2, 4, 6])],
]);
setEnvironmentData("builtins", map);

const worker = new Worker("./builtin-snapshot-worker.cjs");
worker.on("message", (message) => {
  console.log(
    "worker brands:",
    message.map,
    message.date,
    message.set,
  );
  console.log(
    "worker values:",
    message.dateValue,
    message.setValue,
    message.localMutation,
  );
  const parent = getEnvironmentData("builtins");
  console.log("parent unchanged:", parent.has("worker-only"), parent === map);
});
worker.on("exit", (code) => console.log("exit:", code));
