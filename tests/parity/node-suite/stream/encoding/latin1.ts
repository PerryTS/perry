import { Readable } from "node:stream";
// setEncoding('latin1') decodes bytes 1:1 (ISO-8859-1).
const r = new Readable({ read() {} });
r.setEncoding("latin1");
r.on("data", (chunk) => console.log("len:", chunk.length, "first:", chunk.charCodeAt(0)));
r.push(Buffer.from([0xc3, 0xa9])); // é in UTF-8 but two latin1 chars
r.push(null);
