import { ReadableStream, WritableStream } from "node:stream/web";
// pipeTo() takes an internal reader/writer, so during the call rs.locked
// and ws.locked are true; after completion the lock should release.
const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } });
const ws = new WritableStream({ write() {} });
const p = rs.pipeTo(ws);
console.log("rs.locked during:", rs.locked);
console.log("ws.locked during:", ws.locked);
await p;
console.log("rs.locked after:", rs.locked);
console.log("ws.locked after:", ws.locked);
