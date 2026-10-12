// #12285: only an agent's own children keep its loop alive.
import { Worker, isMainThread, parentPort, workerData } from "node:worker_threads";
if (isMainThread) {
  const w = new Worker(new URL(import.meta.url), { workerData: 1 });
  const replies: string[] = [];
  w.on("message", (s: string) => replies.push(s));
  w.on("exit", (code: number) => console.log("nested", code, replies.join(",")));
} else if (workerData === 1) {
  const child = new Worker(new URL(import.meta.url), { workerData: 2 });
  child.once("message", (s: string) => parentPort!.postMessage(s));
} else {
  setTimeout(() => parentPort!.postMessage("child-timer"), 20);
}
