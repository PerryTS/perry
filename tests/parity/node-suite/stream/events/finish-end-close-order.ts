import { Duplex } from "node:stream";
// On a Duplex: 'finish' (write side complete) and 'end' (read side complete)
// fire independently; 'close' fires last after both sides settle.
const events: string[] = [];
const d = new Duplex({
  read() { this.push(null); },
  write(_c, _e, cb) { cb(); },
});
d.on("finish", () => events.push("finish"));
d.on("end", () => events.push("end"));
d.on("close", () => {
  events.push("close");
  console.log("order:", events.join(","));
  console.log("close last:", events[events.length - 1] === "close");
});
d.on("data", () => {});
d.end("x");
