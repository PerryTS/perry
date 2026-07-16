import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();

function make(value: number): ArrayBuffer {
  const buffer = new ArrayBuffer(1);
  new Uint8Array(buffer)[0] = value;
  return buffer;
}

const arrayBuffer = make(1);
const optionsBuffer = make(2);
const iterableBuffer = make(3);

port1.postMessage({ buffer: arrayBuffer }, [arrayBuffer]);
port1.postMessage({ buffer: optionsBuffer }, { transfer: [optionsBuffer] });
port1.postMessage(
  { buffer: iterableBuffer },
  (function* () { yield iterableBuffer; })(),
);

console.log(
  "detached:",
  arrayBuffer.byteLength,
  optionsBuffer.byteLength,
  iterableBuffer.byteLength,
);

const values: string[] = [];
for (let index = 0; index < 3; index += 1) {
  const packet = receiveMessageOnPort(port2);
  const buffer = packet?.message?.buffer;
  values.push(
    buffer instanceof ArrayBuffer
      ? `${buffer.byteLength}:${new Uint8Array(buffer)[0]}`
      : "not-buffer",
  );
}
console.log("received:", values.join(","));

port1.close();
port2.close();
