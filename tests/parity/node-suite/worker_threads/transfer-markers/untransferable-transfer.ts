import {
  isMarkedAsUntransferable,
  markAsUntransferable,
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

const carrier = new MessageChannel();
const buffer = new ArrayBuffer(8);
markAsUntransferable(buffer);
console.log(
  "buffer transfer:",
  isMarkedAsUntransferable(buffer),
  outcome(() => carrier.port1.postMessage(buffer, [buffer])),
  buffer.byteLength,
);

const movable = new MessageChannel();
markAsUntransferable(movable.port1);
console.log(
  "port transfer:",
  isMarkedAsUntransferable(movable.port1),
  outcome(() => carrier.port1.postMessage(movable.port1, [movable.port1])),
);
movable.port1.postMessage("still-owned");
const retainedPacket = receiveMessageOnPort(movable.port2);
console.log(
  "port retained:",
  retainedPacket ? retainedPacket.message : undefined,
);

carrier.port1.close();
carrier.port2.close();
movable.port1.close();
movable.port2.close();
