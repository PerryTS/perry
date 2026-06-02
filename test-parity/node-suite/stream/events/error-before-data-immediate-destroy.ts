import { Readable } from "node:stream";
// If destroy() is called before any data is pushed, only 'error' (if err
// supplied) and 'close' fire; no 'data' or 'end'.
const r = new Readable({ read() {} });
const events: string[] = [];
r.on("data", () => events.push("data"));
r.on("end", () => events.push("end"));
r.on("error", () => events.push("error"));
r.on("close", () => events.push("close"));
r.destroy(new Error("immediate-destroy"));
setImmediate(() => console.log("events:", events.join(",")));
