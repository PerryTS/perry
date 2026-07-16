import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const value = [
  [new SharedArrayBuffer(0), new SharedArrayBuffer(1)],
  [new SharedArrayBuffer(2), new SharedArrayBuffer(3)],
];
const { port1, port2 } = new MessageChannel();

port1.postMessage(value);
const packet = receiveMessageOnPort(port2);
const received = packet?.message;
const flattened = Array.isArray(received) ? received.flat() : [];

console.log(
  "brands:",
  flattened.map((item: any) => item instanceof SharedArrayBuffer).join(","),
);
console.log(
  "lengths:",
  flattened.map((item: any) => item?.byteLength ?? "missing").join(","),
);
console.log(
  "shape:",
  received?.length,
  received?.[0]?.length,
  received?.[1]?.length,
);

port1.close();
port2.close();
