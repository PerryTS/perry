import { Readable } from "node:stream";
// emit('error', err) — listener fires synchronously.
const r = new Readable({ read() {} });
let fired = false;
r.on("error", () => (fired = true));
r.emit("error", new Error("manual-error"));
console.log("fired synchronously:", fired);
