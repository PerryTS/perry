import { ReadableStream } from "node:stream/web";
// If source.cancel() throws, the cancel() Promise rejects.
const rs = new ReadableStream({
  start(c) { c.enqueue("x"); },
  cancel() { throw new Error("cancel-fail"); },
});
let errMsg: string | null = null;
try {
  await rs.cancel();
} catch (e: any) {
  errMsg = e && e.message;
}
console.log("rejected with:", errMsg);
