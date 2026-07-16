import {
  markAsUncloneable,
  MessageChannel,
  receiveMessageOnPort,
} from "node:worker_threads";

function outcome(fn: () => void): string {
  try {
    fn();
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const { port1, port2 } = new MessageChannel();
const root = { value: 1 };
markAsUncloneable(root);
console.log("root:", outcome(() => port1.postMessage(root)));

const nested = { value: 2 };
markAsUncloneable(nested);
console.log(
  "nested:",
  outcome(() => port1.postMessage({ nested })),
);

const buffer = new ArrayBuffer(4);
markAsUncloneable(buffer);
console.log("arraybuffer:", outcome(() => port1.postMessage(buffer)));
const packet = receiveMessageOnPort(port2);
const received = packet ? packet.message : undefined;
console.log(
  "arraybuffer cloned:",
  received instanceof ArrayBuffer,
  received?.byteLength,
  buffer.byteLength,
);

port1.close();
port2.close();
