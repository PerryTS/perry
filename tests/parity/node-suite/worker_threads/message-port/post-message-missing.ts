import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();

try {
  (port1.postMessage as any)();
  console.log("post: ok");
} catch (error: any) {
  console.log("post:", error?.name, error?.code ?? "");
}
console.log("queue:", receiveMessageOnPort(port2));

port1.close();
port2.close();
