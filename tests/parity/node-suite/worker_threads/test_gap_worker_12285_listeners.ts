// #12285: parentPort and worker lifetime must match Node.
import { Worker, isMainThread, parentPort } from "node:worker_threads";
if (isMainThread) {
  const w = new Worker(new URL(import.meta.url));
  const replies: string[] = [];
  w.on("message", (s: string) => replies.push(s));
  w.on("exit", (code: number) => console.log("listeners", code, replies.join(",")));
  w.postMessage(3);
} else {
  const wrong = () => parentPort!.postMessage("wrong");
  parentPort!.on("message", wrong);
  parentPort!.off("message", wrong);
  parentPort!.once("message", (n: number) => parentPort!.postMessage("first:" + n));
  parentPort!.once("message", (n: number) => {
    parentPort!.postMessage("second:" + n);
    setTimeout(() => parentPort!.postMessage("later"), 20);
  });
}
