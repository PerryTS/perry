import { Readable } from "node:stream";
// setEncoding('utf8') makes 'data' events deliver decoded strings instead
// of Buffers.
const r = new Readable({ read() {} });
r.setEncoding("utf8");
r.on("data", (chunk) => console.log("type:", typeof chunk, "value:", chunk));
r.push(Buffer.from("hello"));
r.push(null);
