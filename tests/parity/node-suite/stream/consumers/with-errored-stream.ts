import { Readable } from "node:stream";
import { text } from "node:stream/consumers";
// stream/consumers.text(stream) rejects when the stream errors before end.
const r = new Readable({
  read() { this.emit("error", new Error("kaboom")); },
});
let msg = "";
try { await text(r); } catch (e) { msg = (e as Error).message; }
console.log("rejected:", msg);
