import { Readable, Writable } from "node:stream";
// readableLength / writableLength report buffered byte counts.
const r = new Readable({ read() {} });
r.push(Buffer.from("abc"));
console.log("readableLength:", r.readableLength);

const w = new Writable({
  highWaterMark: 16,
  write(_c, _e, _cb) { /* keep buffered */ },
});
w.write("xyz");
console.log("writableLength typeof number:", typeof w.writableLength === "number");
