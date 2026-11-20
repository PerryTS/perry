import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

function outcome(label: string, fn: () => void) {
  try {
    fn();
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error?.name, error?.code ?? "");
  }
}

const self = new MessageChannel();
outcome("self transfer:", () => self.port1.postMessage(null, [self.port1]));
self.port1.postMessage("usable");
console.log("self retained:", receiveMessageOnPort(self.port2)?.message);

const carrier = new MessageChannel();
const closed = new MessageChannel();
closed.port1.close();
const buffer = new ArrayBuffer(4);
outcome(
  "closed transfer:",
  () => carrier.port1.postMessage({ buffer }, [buffer, closed.port1]),
);
console.log("rollback:", buffer.byteLength);

self.port1.close();
self.port2.close();
carrier.port1.close();
carrier.port2.close();
closed.port2.close();
