import {
  MessageChannel,
  receiveMessageOnPort,
  Worker,
} from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const { port1, port2 } = new MessageChannel();
const worker = new Worker("./workerdata-port-surface-worker.cjs", {
  workerData: { port: port2 },
  transferList: [port2],
});
let summary: any;

worker.on("message", (message) => summary = message);
worker.on("exit", (code) => {
  const packet = receiveMessageOnPort(port1);
  console.log(
    "surface:",
    summary?.brand ?? "missing",
    summary?.methods ?? "missing",
  );
  console.log("peer:", packet ? packet.message : "missing");
  console.log("exit:", code);
  port1.close();
});
