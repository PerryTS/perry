import { Readable } from "node:stream";
import { finished } from "node:stream/promises";
// finished() rejects when the stream errors.
const r = new Readable({ read() {} });
r.on("error", () => {});
setImmediate(() => r.destroy(new Error("test-err")));
let errMsg: string | null = null;
try {
  await finished(r);
} catch (e: any) {
  errMsg = e && e.message;
}
console.log("rejected with:", errMsg);
