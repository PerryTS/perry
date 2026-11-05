import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

function outcome(fn: () => void): string {
  try {
    fn();
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const carrier = new MessageChannel();
console.log(
  "source transfer:",
  outcome(() => carrier.port1.postMessage(null, [carrier.port1])),
);
carrier.port1.postMessage("still-open");
const retained = receiveMessageOnPort(carrier.port2);
console.log("source retained:", retained ? retained.message : undefined);

const closed = new MessageChannel();
closed.port1.close();
console.log(
  "closed transfer:",
  outcome(() => carrier.port1.postMessage(null, [closed.port1])),
);

carrier.port1.close();
carrier.port2.close();
closed.port2.close();
