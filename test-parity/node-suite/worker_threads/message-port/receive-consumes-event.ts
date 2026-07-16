import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const events: string[] = [];

port2.on("message", (message) => {
  events.push(String(message));
  if (message === "barrier") {
    console.log("events:", events.join(","));
    port1.close();
    port2.close();
  }
});

port1.postMessage("synchronous");
const packet = receiveMessageOnPort(port2);
console.log("received:", packet?.message);
port1.postMessage("barrier");
