import { ReadableStream, WritableStream } from "node:stream/web";
// rs.cancel() during an active pipeTo rejects because the readable is locked.
const rs = new ReadableStream({
  async pull(c) {
    await new Promise((resolve) => setTimeout(resolve, 50));
    c.enqueue("x");
    c.close();
  },
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
await p;
