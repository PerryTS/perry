import {
  getEnvironmentData,
  setEnvironmentData,
  Worker,
} from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/environment-data");

setEnvironmentData("nested-key", { level: 0 });
const worker = new Worker("./nested-inheritance-worker.cjs", {
  workerData: { level: 1 },
});

let result: any;
worker.on("message", (message) => {
  result = message;
});
worker.on(
  "error",
  (error: any) => console.log("error:", error?.name, error?.code ?? ""),
);
worker.on("exit", (code) => {
  console.log(
    "levels:",
    result?.first ?? "missing",
    result?.second ?? "missing",
  );
  console.log("parent:", getEnvironmentData("nested-key").level);
  console.log("exit:", code);
});
