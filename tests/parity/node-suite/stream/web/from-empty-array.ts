import { ReadableStream } from "node:stream/web";
// RS.from([]) — yields no data; reader gets {done:true} immediately.
const rs = (ReadableStream as any).from([]);
const reader = rs.getReader();
const result = await reader.read();
console.log("value:", result.value);
console.log("done:", result.done);
