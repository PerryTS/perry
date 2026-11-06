import { Readable } from "node:stream";
// Add listener, emit, listener fires.
const r = new Readable({ read() {} });
let fired = false;
r.on("trigger", () => (fired = true));
r.emit("trigger");
console.log("fired:", fired);
