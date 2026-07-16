import { MessagePort, receiveMessageOnPort, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

let receivedPort: any;
const worker = new Worker("./worker-created-port-worker.cjs");
worker.on("message", (value: any) => {
  receivedPort = value?.port;
});
worker.on("error", (error: any) => {
  console.log("error:", error?.name, error?.code ?? "");
});
worker.on("exit", (code) => {
  let message: any = "missing";
  try {
    message = receiveMessageOnPort(receivedPort)?.message ?? "empty";
  } catch (error: any) {
    message = `${error?.name}:${error?.code ?? ""}`;
  }
  console.log("port:", receivedPort instanceof MessagePort, message);
  console.log("exit:", code);
  receivedPort?.close?.();
});
