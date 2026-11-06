import {
  MessageChannel,
  MessagePort,
  receiveMessageOnPort,
} from "node:worker_threads";

const carrier = new MessageChannel();
const movable = new MessageChannel();

carrier.port1.postMessage({ port: movable.port1 }, [movable.port1]);
const packet = receiveMessageOnPort(carrier.port2);
const transferred = packet ? packet.message?.port : undefined;
console.log("transferred type:", transferred instanceof MessagePort);

if (typeof transferred?.postMessage === "function") {
  transferred.postMessage("new-owner");
}
const newOwnerPacket = receiveMessageOnPort(movable.port2);
console.log(
  "new owner delivery:",
  newOwnerPacket ? newOwnerPacket.message : undefined,
);

movable.port1.postMessage("old-owner");
const oldOwnerPacket = receiveMessageOnPort(movable.port2);
console.log(
  "old owner detached:",
  oldOwnerPacket ? oldOwnerPacket.message : undefined,
);

if (typeof transferred?.close === "function") {
  transferred.close();
}
movable.port1.close();
movable.port2.close();
carrier.port1.close();
carrier.port2.close();
