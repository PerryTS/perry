import { Readable } from "node:stream";
// destroy(err) on a Readable WITH an error listener stays in the program;
// without one would crash via uncaughtException (we install one here).
const r = new Readable({ read() {} });
let msg = "";
r.on("error", (e) => { msg = (e as Error).message; });
r.destroy(new Error("delivered"));
setImmediate(() => console.log("captured:", msg));
