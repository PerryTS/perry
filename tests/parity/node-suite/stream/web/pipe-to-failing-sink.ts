import { ReadableStream, WritableStream } from "node:stream/web";
// pipeTo() to a sink whose write() throws — Promise rejects.
const rs = new ReadableStream({
  start(c) { c.enqueue("x"); c.close(); },
});
const ws = new WritableStream({
  write() { throw new Error("sink-fail"); },
});
let errMsg: string | null = null;
try {
  await rs.pipeTo(ws);
} catch (e: any) {
  errMsg = e && e.message;
}
console.log("rejected with:", errMsg);
