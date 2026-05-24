import { ReadableStream, TransformStream } from "node:stream/web";
// pipeThrough → then cancel the output. Source upstream should be cancelled too.
const rs = new ReadableStream({
  pull(c) { setTimeout(() => c.enqueue("x"), 30); },
});
const ts = new TransformStream();
const result = rs.pipeThrough(ts);
let cancelOk = false;
try {
  await result.cancel("downstream");
  cancelOk = true;
} catch {
  cancelOk = false;
}
console.log("cancel ok:", cancelOk);
console.log("result locked:", result.locked);
