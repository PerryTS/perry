import { Readable } from "node:stream";
// destroy(non-Error) — Node accepts any value, emits 'error' with the
// raw value (not wrapped in an Error).
const r = new Readable({ read() {} });
let received: any = null;
r.on("error", (err) => (received = err));
r.destroy("string-not-error" as any);
setImmediate(() => {
  console.log("received:", received);
  console.log("is string:", typeof received === "string");
});
