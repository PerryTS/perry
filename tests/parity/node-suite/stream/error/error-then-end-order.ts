import { Readable } from "node:stream";
// Once a stream emits 'error', 'end' is NOT subsequently emitted — error
// supersedes normal completion.
let order = "";
const r = new Readable({ read() {} });
r.on("data", () => {});
r.on("error", () => (order += "E"));
r.on("end", () => (order += "X"));
r.destroy(new Error("stop"));
setImmediate(() =>
  setImmediate(() => console.log("order:", order))
);
