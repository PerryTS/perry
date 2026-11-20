import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const source = new SharedArrayBuffer(8);
const sourceView = new Int32Array(source);
Atomics.store(sourceView, 0, 10);

port1.postMessage(source);
const packet = receiveMessageOnPort(port2);
const received = packet ? packet.message : undefined;
const receivedView = received instanceof SharedArrayBuffer
  ? new Int32Array(received)
  : undefined;

console.log(
  "brand/identity:",
  received instanceof SharedArrayBuffer,
  received === source,
  received?.byteLength,
);
Atomics.store(sourceView, 0, 42);
console.log("shared update:", receivedView ? Atomics.load(receivedView, 0) : "missing");

port1.close();
port2.close();
