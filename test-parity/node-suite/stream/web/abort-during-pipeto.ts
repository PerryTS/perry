import { ReadableStream, WritableStream } from "node:stream/web";
// Aborting the writable side during an active pipeTo — pipeTo rejects.
const rs = new ReadableStream({
  pull(c) { setTimeout(() => c.enqueue("x"), 30); },
});
const ws = new WritableStream({ write() {} });
const p = rs.pipeTo(ws);
setTimeout(() => {
  ws.abort("stop").catch(() => {});
}, 5);
let rejected = false;
try {
  await p;
} catch {
  rejected = true;
}
console.log("pipeTo rejected after abort:", rejected);
