import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const buffer = new ArrayBuffer(8);
const view = new Uint32Array(buffer);
view[0] = 0x12345678;

port1.postMessage(view, [buffer]);
console.log("detached:", buffer.byteLength, view.byteLength);

try {
  port1.postMessage(view, [buffer]);
  console.log("retransfer: ok");
} catch (error: any) {
  console.log("retransfer:", error?.name, error?.code ?? "");
}

const first = receiveMessageOnPort(port2);
console.log(
  "first:",
  first?.message instanceof Uint32Array,
  first?.message?.[0],
);
console.log("second:", receiveMessageOnPort(port2));

port1.close();
port2.close();
