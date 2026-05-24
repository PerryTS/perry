import { Readable } from "node:stream";
// Calling pause() inside a 'data' listener stops further synchronous emits.
const r = Readable.from(["a", "b", "c", "d"]);
const out: string[] = [];
r.on("data", (c) => {
  out.push(String(c));
  r.pause();
});
setImmediate(() => {
  console.log("first batch count:", out.length);
  console.log("flowing:", r.readableFlowing);
});
