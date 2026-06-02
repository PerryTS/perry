import { WritableStream } from "node:stream/web";
// writer.write(chunk) returns a Promise that resolves once the chunk is
// consumed by the sink.
const ws = new WritableStream({ write() {} });
const w = ws.getWriter();
const ret = w.write("x");
console.log("returns promise:", typeof (ret as any).then === "function");
await ret;
console.log("resolved");
