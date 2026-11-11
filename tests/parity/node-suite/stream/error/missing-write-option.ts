import { Writable } from "node:stream";
// new Writable() (no write option, no subclass override) must throw or
// invoke its error path when write() is called — Node defaults to firing
// 'error' with ERR_METHOD_NOT_IMPLEMENTED.
const w = new Writable();
let fired = false;
w.on("error", () => (fired = true));
w.write("x");
setImmediate(() => console.log("error fired:", fired));
