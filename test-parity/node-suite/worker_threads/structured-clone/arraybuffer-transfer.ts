import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const buffer = new ArrayBuffer(8);
const view = new Uint32Array(buffer);
view.set([0x12345678, 0x90abcdef]);

port1.postMessage(view, [buffer]);
console.log("detached:", buffer.byteLength, view.byteLength, view.length);

const packet = receiveMessageOnPort(port2);
const received = packet ? packet.message : undefined;
console.log(
  "received:",
  received instanceof Uint32Array,
  received?.byteLength,
  received?.length,
  received?.[0]?.toString(16),
  received?.[1]?.toString(16),
);

port1.close();
port2.close();
