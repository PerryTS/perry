import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const closing = new MessageChannel();
const carrier = new MessageChannel();
let deliveries = 0;
let transfer = "missing";

closing.port1.on("message", () => {
  deliveries += 1;
  if (deliveries === 1) {
    closing.port1.close();
    return;
  }

  try {
    carrier.port1.postMessage(null, [closing.port1]);
    transfer = "ok";
  } catch (error: any) {
    transfer = `${error?.name}:${error?.code ?? ""}`;
  }
});

closing.port1.on("close", () => {
  console.log("deliveries:", deliveries);
  console.log("transfer:", transfer);
  console.log("carrier queue:", receiveMessageOnPort(carrier.port2));
  closing.port2.close();
  carrier.port1.close();
  carrier.port2.close();
});

closing.port2.postMessage("first");
closing.port2.postMessage("second");
