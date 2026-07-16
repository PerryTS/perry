import { MessageChannel, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const { port1, port2 } = new MessageChannel();
port1.postMessage({ text: "queued", count: 2 });

const worker = new Worker("./queued-port-receive-worker.cjs", {
  workerData: { port: port2 },
  transferList: [port2],
});

worker.on("message", (message) => {
  if (message.closeError) {
    console.log("close error:", message.closeError);
    return;
  }
  console.log("received:", message.text, message.count, message.empty);
});
worker.on(
  "error",
  (error: any) => console.log("error:", error?.name, error?.code ?? ""),
);
worker.on("exit", (code) => {
  console.log("exit:", code);
  port1.close();
});
