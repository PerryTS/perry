// #12285: parentPort and worker lifetime must match Node.
import { Worker, isMainThread, parentPort } from "node:worker_threads";
if (isMainThread) {
  const w = new Worker(new URL(import.meta.url));
  const replies: string[] = [];
  w.on("message", (n: string) => replies.push(n));
  w.on("exit", (code: number) => console.log("once", code, replies.join(",")));
  w.postMessage(7);
  w.postMessage(9);
} else {
  parentPort!.once("message", function(this: any, n: number) {
    parentPort!.postMessage([n * 2, parentPort!.listenerCount("message"), parentPort!.hasRef(), this === parentPort].join(":"));
  });
}
