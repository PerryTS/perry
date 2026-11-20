import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const carrier = new MessageChannel();
const movable = new MessageChannel();

carrier.port2.addEventListener("message", (event: MessageEvent) => {
  const transferred = event.data?.port;
  const ports = Array.isArray(event.ports) ? event.ports : [];
  console.log(
    "event ports:",
    ports.length,
    ports[0] === transferred,
    typeof transferred?.postMessage,
  );
  if (typeof transferred?.postMessage === "function") {
    transferred.postMessage("via-event-port");
  }
  const packet = receiveMessageOnPort(movable.port2);
  console.log("delivery:", packet ? packet.message : undefined);

  if (typeof transferred?.close === "function") transferred.close();
  carrier.port1.close();
  carrier.port2.close();
  movable.port1.close();
  movable.port2.close();
});
carrier.port2.start();
carrier.port1.postMessage({ port: movable.port1 }, [movable.port1]);
