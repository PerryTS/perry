import { Readable } from "node:stream";
// 'error' event listener receives the exact Error instance passed to destroy().
const orig = new Error("specific-instance");
let received: any = null;
const r = new Readable({ read() {} });
r.on("error", (e) => (received = e));
r.destroy(orig);
setImmediate(() => {
  console.log("identity match:", received === orig);
  console.log("instanceof Error:", received instanceof Error);
  console.log("message:", received && (received as Error).message);
});
