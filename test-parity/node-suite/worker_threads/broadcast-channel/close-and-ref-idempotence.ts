import { BroadcastChannel } from "node:worker_threads";

const channel = new BroadcastChannel("idempotent");
console.log("ref return:", channel.ref() === channel);
console.log("unref return:", channel.unref() === channel);
console.log("ref again:", channel.ref() === channel);
console.log("close return:", channel.close());
console.log("close twice:", channel.close());

try {
  channel.postMessage("closed");
  console.log("post closed: ok");
} catch (error: any) {
  console.log("post closed:", error?.name, error?.code ?? "");
}
