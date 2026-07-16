import path from "node:path";
import { pathToFileURL } from "node:url";
import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const filename = path.resolve("url-π-worker.cjs");
try {
  const worker = new Worker(pathToFileURL(filename), {
    workerData: "unicode-ok",
  });

  worker.on("message", (message) => console.log("message:", message));
  worker.on(
    "error",
    (error: any) => console.log("error:", error?.name, error?.code ?? ""),
  );
  worker.on("exit", (code) => console.log("exit:", code));
} catch (error: any) {
  console.log("construct:", error?.name, error?.code ?? "");
}
