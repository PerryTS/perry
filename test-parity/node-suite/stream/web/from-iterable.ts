import { ReadableStream } from "node:stream/web";
// WHATWG ReadableStream.from(iterable) (Node 20+) builds a Web ReadableStream
// from any (a)sync iterable.
const rs = (ReadableStream as any).from(["a", "b"]);
const reader = rs.getReader();
const a = await reader.read();
const b = await reader.read();
console.log("a:", a.value, "b:", b.value);
