import { Readable } from "node:stream";
// setEncoding('hex') decodes each Buffer chunk to hex.
const r = new Readable({ read() {} });
r.setEncoding("hex");
r.on("data", (chunk) => console.log("hex:", chunk));
r.push(Buffer.from([0xab, 0xcd]));
r.push(null);
