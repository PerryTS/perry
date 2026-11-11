// #12285: parentPort and worker lifetime must match Node.
import { Worker, isMainThread, parentPort } from "node:worker_threads";
if (isMainThread) {
  const w = new Worker(new URL(import.meta.url));
  const replies: string[] = [];
  w.on("message", (s: string) => replies.push(s));
  w.on("exit", (code: number) => console.log("ref", code, replies.join(",")));
} else {
  const states: boolean[] = [parentPort!.hasRef()];
  const handler = () => {};
  parentPort!.on("message", handler);
  states.push(parentPort!.hasRef());
  parentPort!.unref();
  states.push(parentPort!.hasRef());
  parentPort!.ref();
  states.push(parentPort!.hasRef());
  parentPort!.off("message", handler);
  states.push(parentPort!.hasRef());
  parentPort!.on("message", handler);
  parentPort!.unref();
  parentPort!.postMessage(states.join(","));
}
