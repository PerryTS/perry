import { TransformStream } from "node:stream/web";
// Cancelling the readable side aborts the writable side.
const ts = new TransformStream();
const reader = ts.readable.getReader();
await reader.cancel("downstream-stop");
const writer = ts.writable.getWriter();
let writeRejected = false;
try {
  await writer.write("x");
} catch {
  writeRejected = true;
}
console.log("write rejected after readable cancel:", writeRejected);
