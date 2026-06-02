import { Readable } from "node:stream";
// Listener for 'error' receives the exact Error instance from destroy.
const r = new Readable({ read() {} });
const original = new Error("test");
let received: any = null;
r.on("error", (e) => (received = e));
r.destroy(original);
setImmediate(() => {
  console.log("instanceof Error:", received instanceof Error);
  console.log("same identity:", received === original);
  console.log("message:", received && (received as Error).message);
});
