import {
  markAsUncloneable,
  MessageChannel,
  receiveMessageOnPort,
} from "node:worker_threads";

const carrier = new MessageChannel();
const movable = new MessageChannel();
markAsUncloneable(movable.port1);

try {
  carrier.port1.postMessage({ port: movable.port1 }, [movable.port1]);
  console.log("transfer: ok");
} catch (error: any) {
  console.log("transfer:", error?.name, error?.code ?? "");
}

const packet = receiveMessageOnPort(carrier.port2);
console.log("received port:", packet?.message?.port instanceof MessagePort);
if (typeof packet?.message?.port?.postMessage === "function") {
  packet.message.port.postMessage("through-transfer");
}
console.log("peer:", receiveMessageOnPort(movable.port2)?.message);

packet?.message?.port?.close?.();
carrier.port1.close();
carrier.port2.close();
movable.port2.close();
