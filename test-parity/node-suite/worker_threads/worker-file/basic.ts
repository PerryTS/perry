import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-file");

const worker = new Worker("./worker.cjs", {
  workerData: { label: "perry", count: 41 },
  name: "probe-worker",
});

let sawStartup = false;
let sawReply = false;

worker.on("online", () => {
  console.log("online");
});

worker.on("message", (message) => {
  if (message.phase === "startup") {
    sawStartup = true;
    console.log("startup:", message.main, message.value);
    console.log("threadId number:", typeof worker.threadId);
    console.log("threadName:", worker.threadName);
    console.log("resourceLimits keys:", Object.keys(worker.resourceLimits).length);
    console.log("methods:", typeof worker.ref, typeof worker.unref, typeof worker.terminate);
    worker.unref();
    console.log("unref return");
    worker.ref();
    console.log("ref return");
    worker.postMessage({ value: 10, nested: { ok: true } });
    return;
  }

  if (message.phase === "reply") {
    sawReply = true;
    console.log("reply:", message.value, message.nestedOk);
    worker.terminate().then((code) => {
      console.log("terminate:", code);
    });
  }
});

worker.on("exit", (code) => {
  console.log("exit:", code, sawStartup, sawReply);
});
