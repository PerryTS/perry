import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const events: string[] = [];
let closeEvents = 0;

function closed(label: string) {
  events.push(label);
  closeEvents += 1;
  if (closeEvents !== 2) return;
  setImmediate(() => {
    console.log("close events:", events.sort().join(","));
    console.log("closed methods:", port1.start(), port1.ref(), port1.unref());
  });
}

port1.on("close", () => closed("port1-event"));
port2.on("close", () => closed("port2-event"));
port1.close(() => events.push("port1-callback"));
port1.close(() => events.push("second-callback"));
port2.close();
