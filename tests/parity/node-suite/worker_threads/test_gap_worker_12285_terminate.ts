// #12285: parentPort and worker lifetime must match Node.
import { Worker, isMainThread, parentPort } from "node:worker_threads";
if (isMainThread) {
  const w = new Worker(new URL(import.meta.url));
  w.on("exit", (code: number) => console.log("exit", code));
  w.once("message", () => {
    w.terminate().then((code: number) => console.log("terminated", code));
  });
} else {
  parentPort!.on("message", () => {});
  parentPort!.postMessage("ready");
}
