// #12285: parentPort and worker lifetime must match Node.
import { Worker, isMainThread, parentPort } from "node:worker_threads";
if (isMainThread) {
  const w = new Worker(new URL(import.meta.url));
  const replies: string[] = [];
  w.on("message", (s: string) => replies.push(s));
  w.on("exit", (code: number) => console.log("async", code, replies.join(",")));
} else {
  setTimeout(() => {
    parentPort!.postMessage("timer");
    Promise.resolve().then(() => parentPort!.postMessage("job"));
  }, 20);
}
