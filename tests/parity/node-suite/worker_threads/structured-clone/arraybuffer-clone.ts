import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const buffer = new ArrayBuffer(8);
const view = new Uint16Array(buffer);
view.set([0x1234, 0x5678]);

port1.postMessage({ buffer, view });
view[0] = 0xffff;

const packet = receiveMessageOnPort(port2);
const received = packet?.message;
console.log(
  "clone shape:",
  received?.buffer instanceof ArrayBuffer,
  received?.view instanceof Uint16Array,
  received?.view?.buffer === received?.buffer,
);
console.log("clone values:", received?.view?.[0], received?.view?.[1]);
console.log("source retained:", buffer.byteLength, view[0]);

port1.close();
port2.close();
