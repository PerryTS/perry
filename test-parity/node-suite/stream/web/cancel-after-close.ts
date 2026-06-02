import { ReadableStream } from "node:stream/web";
// cancel() after the stream has closed — resolves to undefined.
const rs = new ReadableStream({ start(c) { c.close(); } });
let result: any = "untouched";
try {
  result = await rs.cancel("after-close");
} catch (e: any) {
  result = "threw:" + e.name;
}
console.log("result:", result);
console.log("is undefined:", result === undefined);
