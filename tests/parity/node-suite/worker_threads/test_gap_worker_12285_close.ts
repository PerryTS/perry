// #12285: parentPort and worker lifetime must match Node.
import { Worker, isMainThread, parentPort } from "node:worker_threads";
if (isMainThread) {
  const w = new Worker(new URL(import.meta.url));
  const replies: string[] = [];
  w.on("message", (s: string) => replies.push(s));
  w.on("exit", (code: number) => console.log("close", code, replies.join(",")));
  w.postMessage("go");
} else {
  let closed = false;
  parentPort!.once("close", function(this: any, ...args: any[]) {
    closed = args.length === 1 && args[0].type === "close"
      && args[0].target === parentPort && this === parentPort;
  });
  parentPort!.on("message", () => {
    parentPort!.postMessage("reply");
    parentPort!.close();
    parentPort!.postMessage("after-close");
    setTimeout(() => console.log("close-event", closed), 20);
  });
}
