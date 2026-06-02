import { ReadableStream, WritableStream } from "node:stream/web";
// pipeTo() with no options arg — defaults work.
const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } });
const ws = new WritableStream({ write() {} });
await rs.pipeTo(ws);
console.log("rs locked after:", rs.locked);
console.log("ws locked after:", ws.locked);
