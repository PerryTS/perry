import { Readable } from "node:stream";
// destroy() with a non-Error value still tears down the stream (error event
// receives the value as-is).
const r = new Readable({ read() {} });
let got: any = null;
r.on("error", (e: any) => (got = e));
r.destroy(Symbol("nope") as any);
setImmediate(() => console.log("type:", typeof got));
