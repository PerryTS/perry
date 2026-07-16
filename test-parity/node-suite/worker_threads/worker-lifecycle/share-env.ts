import { SHARE_ENV, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

process.env.PERRY_SHARED_PARENT = "initial";
delete process.env.PERRY_SHARED_WORKER;

const worker = new Worker("./share-env-worker.cjs", { env: SHARE_ENV });
worker.on("message", (message: any) => {
  if (message.phase === "ready") {
    console.log("worker initial:", message.parent);
    process.env.PERRY_SHARED_PARENT = "updated";
    worker.postMessage("check");
    return;
  }

  console.log("worker updated:", message.parent);
  console.log("parent sees worker:", process.env.PERRY_SHARED_WORKER);
  worker.terminate().then((code) => console.log("terminate:", code));
});
worker.on("exit", (code) => console.log("exit:", code));
