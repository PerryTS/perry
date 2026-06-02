import { Readable } from "node:stream";
// read() with no argument returns the entire buffered content as one chunk.
const r = new Readable({ read() {} });
r.push(Buffer.from("hello"));
r.push(null);
const chunk = r.read();
console.log("length:", chunk && chunk.length);
console.log("content:", chunk && chunk.toString());
