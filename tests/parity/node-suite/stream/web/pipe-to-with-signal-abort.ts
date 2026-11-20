import { ReadableStream, WritableStream } from "node:stream/web";
// pipeTo(ws, { signal }) — aborting the signal rejects the returned Promise.
const ctrl = new AbortController();
const rs = new ReadableStream({
  pull(c) {
    setTimeout(() => c.enqueue("x"), 50);
  },
});
const ws = new WritableStream({ write() {} });
const p = rs.pipeTo(ws, { signal: ctrl.signal });
setTimeout(() => ctrl.abort(), 10);
let err: string | null = null;
try {
  await p;
} catch (e: any) {
  err = e && e.name;
}
console.log("aborted err name:", err);
