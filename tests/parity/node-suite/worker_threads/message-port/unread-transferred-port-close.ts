import { MessageChannel } from "node:worker_threads";

const carrier = new MessageChannel();
const movable = new MessageChannel();

movable.port2.ref();
movable.port2.on("close", () => {
  console.log("peer closed");
  movable.port2.close();
  carrier.port1.close();
});

carrier.port1.postMessage(movable.port1, [movable.port1]);
carrier.port2.close();
