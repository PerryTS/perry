import { ReadableStream, WritableStream } from "node:stream/web";
// rs.cancel() during an active pipeTo — the pipeTo promise rejects (locked stream).
const rs = new ReadableStream({
  pull(c) { setTimeout(() => c.enqueue("x"), 50); },
});
const ws = new WritableStream({ write() {} });
const p = rs.pipeTo(ws);
// While locked, cancel directly errors
let cancelErr: string | null = null;
try {
  await rs.cancel("manual");
} catch (e: any) {
  cancelErr = e && e.name;
}
console.log("cancel-on-locked rejected:", cancelErr);
// pipeTo will likely still be pending
p.catch(() => {});
