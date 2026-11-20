import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const events: string[] = [];
let closes = 0;

function closed(side: string) {
  events.push(`${side}-close`);
  closes += 1;
  if (closes === 2) {
    console.log("events:", events.join(","));
    port2.postMessage("after-close");
    const packet = receiveMessageOnPort(port1);
    console.log("after close:", packet?.message);
  }
}

port1.on("close", () => closed("port1"));
port2.on("message", (message) => events.push(`message:${message}`));
port2.on("close", () => closed("port2"));

port1.postMessage("before-close");
port1.close();
