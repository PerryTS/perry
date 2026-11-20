import { Readable } from "node:stream";
// Calling Symbol.dispose on a stream destroys it synchronously (Node 21+).
const r = new Readable({ read() {} });
r.on("error", () => {});
const dispose = (r as any)[Symbol.dispose];
if (typeof dispose === "function") {
  dispose.call(r);
  setImmediate(() => {
    console.log("destroyed after Symbol.dispose:", r.destroyed);
  });
} else {
  console.log("Symbol.dispose not available, typeof:", typeof dispose);
}
