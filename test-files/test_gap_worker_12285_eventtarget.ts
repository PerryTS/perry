// #12285: EventTarget once listeners share MessagePort lifetime semantics.
import { Worker, isMainThread, parentPort } from "node:worker_threads";
if (isMainThread) {
  const w = new Worker(new URL(import.meta.url));
  const replies: string[] = [];
  w.on("message", (s: string) => replies.push(s));
  w.on("exit", (code: number) => console.log("eventtarget", code, replies.join(",")));
  w.postMessage(5);
  w.postMessage(6);
} else {
  parentPort!.addEventListener("message", (event: any) => {
    parentPort!.postMessage([event.data, event.target === parentPort, parentPort!.hasRef()].join(":"));
  }, { once: true });
}
