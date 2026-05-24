import { TransformStream } from "node:stream/web";
// A throw in transform() aborts both the readable and writable sides;
// further reads should reject.
const ts = new TransformStream({
  transform() { throw new Error("xform-boom"); },
});
const writer = ts.writable.getWriter();
const reader = ts.readable.getReader();
let writeErr: string | null = null;
let readErr: string | null = null;
try {
  await writer.write("x");
} catch (e: any) {
  writeErr = e && e.message;
}
try {
  await reader.read();
} catch (e: any) {
  readErr = e && e.message;
}
console.log("write rejected:", writeErr);
console.log("read rejected:", readErr);
