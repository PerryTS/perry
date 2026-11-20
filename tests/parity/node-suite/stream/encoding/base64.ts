import { Readable } from "node:stream";
// setEncoding('base64') decodes each Buffer chunk to base64.
const r = new Readable({ read() {} });
r.setEncoding("base64");
r.on("data", (chunk) => console.log("b64:", chunk));
r.push(Buffer.from("hello"));
r.push(null);
