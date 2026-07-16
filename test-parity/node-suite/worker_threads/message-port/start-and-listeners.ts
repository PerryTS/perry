import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const messages: string[] = [];

function listener(event: MessageEvent) {
  messages.push(String(event.data));
  if (messages.length === 2) {
    console.log("delivered:", messages.join(","));
    port1.close();
    port2.close();
  }
}

port1.addEventListener("message", listener);
port1.addEventListener("message", listener);
port2.postMessage("first");
port2.postMessage("second");
console.log("before start:", messages.length);
console.log("start return:", port1.start());
