import { setEnvironmentData, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/environment-data");

try {
  setEnvironmentData("uncloneable", () => "value");
  new Worker("./worker-snapshot-worker.cjs");
  console.log("construct: ok");
} catch (error: any) {
  console.log("construct:", error?.name, error?.code ?? "");
}

setEnvironmentData("uncloneable", undefined);
const worker = new Worker("./environment-clean-worker.cjs");
worker.on("message", (message) => console.log("after delete:", message));
worker.on("exit", (code) => console.log("exit:", code));
