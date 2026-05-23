import { WritableStream } from "node:stream/web";
// writer.desiredSize is the available backpressure budget; positive when
// the buffer has room.
const ws = new WritableStream({ write() {} }, { highWaterMark: 5 });
const w = ws.getWriter();
console.log("typeof number:", typeof w.desiredSize === "number");
console.log("initially positive:", (w.desiredSize ?? 0) > 0);
