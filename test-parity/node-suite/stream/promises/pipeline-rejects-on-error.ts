import { Readable, PassThrough } from "node:stream";
import { pipeline } from "node:stream/promises";
// stream/promises.pipeline rejects when a stage errors.
const r = new Readable({ read() { this.emit("error", new Error("nope")); } });
const sink = new PassThrough();
let msg = "";
try { await pipeline(r, sink); } catch (e) { msg = (e as Error).message; }
console.log("rejected msg:", msg);
