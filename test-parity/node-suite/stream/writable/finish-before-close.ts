import { Writable } from "node:stream";
// On a Writable, 'finish' fires before 'close'.
const order: string[] = [];
const w = new Writable({ write(_c, _e, cb) { cb(); } });
w.on("finish", () => order.push("finish"));
w.on("close", () => order.push("close"));
w.end(() => {
  setImmediate(() => console.log("order:", order.join(",")));
});
