import { ReadableStream, WritableStream } from "node:stream/web";

// Direct WritableStream.abort() is rejected while pipeTo owns the destination
// writer lock. The pipe itself still completes once the source closes.
const rs = new ReadableStream({
  start(c) {
    setTimeout(() => {
      c.enqueue("x");
      c.close();
    }, 30);
  },
});
const ws = new WritableStream({ write() {} });
const pipe = rs.pipeTo(ws).then(
  () => "resolved",
  (e: any) => `rejected ${e?.name || e}`,
);

console.log("locked during pipe:", rs.locked, ws.locked);

const abortResult = new Promise<string>((resolve) => {
  setTimeout(async () => {
    try {
      await ws.abort("stop");
      resolve("resolved");
    } catch (e: any) {
      resolve(`${e?.name} ${e?.message}`);
    }
  }, 5);
});

console.log("direct abort during pipe:", await abortResult);
console.log("pipe result:", await pipe);
console.log("locked after pipe:", rs.locked, ws.locked);

let sinkAbortReason = "unset";
const writerStream = new WritableStream({
  abort(reason) {
    sinkAbortReason = String(reason);
  },
});
const writer = writerStream.getWriter();
await writer.abort("writer-stop");
console.log("writer abort reason:", sinkAbortReason);
console.log("writer stream locked:", writerStream.locked);
