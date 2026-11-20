import { Writable } from "node:stream";
// On Writable: 'finish' fires before 'close'.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
const events: string[] = [];
w.on("finish", () => events.push("finish"));
w.on("close", () => {
  events.push("close");
  console.log("order:", events.join(","));
  console.log("finish before close:", events.indexOf("finish") < events.indexOf("close"));
});
w.end("x");
