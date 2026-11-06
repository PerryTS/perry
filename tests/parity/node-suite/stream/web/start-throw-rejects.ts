import { ReadableStream } from "node:stream/web";
// If the underlying source's start() throws (or returns a rejected
// Promise), the stream errors and reads/cancel reject.
const rs = new ReadableStream({
  start() {
    throw new Error("start-fail");
  },
});
const reader = rs.getReader();
let errMsg: string | null = null;
try {
  await reader.read();
} catch (e: any) {
  errMsg = e && e.message;
}
console.log("read rejected:", errMsg);
