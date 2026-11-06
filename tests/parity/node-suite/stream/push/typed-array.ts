import { Readable } from "node:stream";
// push(Uint8Array) delivers the chunk as a Buffer view.
const r = new Readable({ read() {} });
r.on("data", (chunk) => {
  console.log("is buffer:", Buffer.isBuffer(chunk));
  console.log("byte 0:", chunk[0]);
});
r.push(new Uint8Array([42, 43]));
r.push(null);
