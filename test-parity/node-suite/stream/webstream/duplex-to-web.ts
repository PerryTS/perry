import { Duplex } from "node:stream";
// Duplex.toWeb returns { readable, writable } and forwards writable input to
// readable output for a custom duplex.
const d = new Duplex({
  objectMode: true,
  read() {},
  write(c, _e, cb) {
    this.push(c);
    cb();
  },
});
const pair = (Duplex as any).toWeb(d);
console.log("readable:", typeof pair.readable === "object" && typeof pair.readable.getReader === "function");
console.log("writable:", typeof pair.writable === "object" && typeof pair.writable.getWriter === "function");
const writer = pair.writable.getWriter();
const reader = pair.readable.getReader();
await writer.write("x");
console.log("chunk:", (await reader.read()).value);
