import { ReadableStream } from "node:stream/web";
// controller.error(reason) puts the stream into the errored state.
let triggered = false;
const rs = new ReadableStream({
  start(c) { c.error(new Error("ctl-err")); },
});
const r = rs.getReader();
try { await r.read(); } catch { triggered = true; }
console.log("read rejected:", triggered);
