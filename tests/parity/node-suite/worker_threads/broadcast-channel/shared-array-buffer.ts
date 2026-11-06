import {
  BroadcastChannel,
  receiveMessageOnPort,
} from "node:worker_threads";

const sender = new BroadcastChannel("shared-buffer");
const receiver = new BroadcastChannel("shared-buffer");
const source = new SharedArrayBuffer(4);
const sourceView = new Int32Array(source);
Atomics.store(sourceView, 0, 5);

sender.postMessage(source);
const packet = receiveMessageOnPort(receiver);
const received = packet ? packet.message : undefined;
const receivedView = received instanceof SharedArrayBuffer
  ? new Int32Array(received)
  : undefined;

console.log("brand/identity:", received instanceof SharedArrayBuffer, received === source);
Atomics.store(sourceView, 0, 27);
console.log("shared update:", receivedView ? Atomics.load(receivedView, 0) : "missing");

sender.close();
receiver.close();
