import { MessageChannel, Worker, isMainThread, parentPort } from "node:worker_threads";

if (!isMainThread) {
  const previous = parentPort!;
  const queued = new MessageChannel();
  queued.port1.postMessage("queued");
  const child = new Worker(new URL("./transfer-child-worker.ts", import.meta.url), {
    workerData: {
      endpoint: previous,
      alias: previous,
      extraQueued: queued.port2,
    },
    transferList: [previous, queued.port2],
  });
  child.once("message", () => child.postMessage("doneACK"));
  child.on("exit", () => queued.port1.close());
  previous.postMessage("stale");
  previous.start();
  previous.ref();
  previous.unref();
  previous.close();
}
