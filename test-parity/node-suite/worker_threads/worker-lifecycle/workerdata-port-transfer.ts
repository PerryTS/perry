import {
  MessageChannel,
  receiveMessageOnPort,
  Worker,
} from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const channel = new MessageChannel();
const worker = new Worker("./workerdata-port-worker.cjs", {
  workerData: { port: channel.port1 },
  transferList: [channel.port1],
});

worker.on("message", (message: any) => {
  const packet = receiveMessageOnPort(channel.port2);
  console.log("worker port:", message?.brand, message?.postMessage);
  console.log("peer delivery:", packet ? packet.message : undefined);
  channel.port1.postMessage("old-owner");
  const oldOwner = receiveMessageOnPort(channel.port2);
  console.log("old owner detached:", oldOwner ? oldOwner.message : undefined);
  worker.terminate();
});
worker.on("exit", (code) => {
  console.log("exit:", code);
  channel.port1.close();
  channel.port2.close();
});
