import { Readable } from "node:stream";
// push(Buffer) delivers a Buffer chunk on 'data' (Buffer instance, length).
const r = new Readable({ read() {} });
r.on("data", (chunk) => {
  console.log("is buffer:", Buffer.isBuffer(chunk));
  console.log("length:", chunk.length);
});
r.push(Buffer.from("hello"));
r.push(null);
