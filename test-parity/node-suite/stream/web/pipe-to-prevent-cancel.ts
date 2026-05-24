import { ReadableStream, WritableStream } from "node:stream/web";
// pipeTo(ws, { preventCancel: true }) prevents the source from being
// cancelled if the destination errors.
const rs = new ReadableStream({
  start(c) { c.enqueue("x"); c.close(); },
});
const ws = new WritableStream({ write() {} });
const p = rs.pipeTo(ws, { preventCancel: true });
console.log("returns promise:", typeof (p as any).then === "function");
await p;
console.log("rs.locked after:", rs.locked);
