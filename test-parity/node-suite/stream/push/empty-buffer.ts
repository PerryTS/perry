import { Readable } from "node:stream";
// push(empty Buffer) is a no-op (no data event, doesn't end the stream).
const r = new Readable({ read() {} });
let dataCount = 0;
r.on("data", () => dataCount++);
r.on("end", () => console.log("data count:", dataCount));
r.push(Buffer.alloc(0));
r.push(null);
