import { ReadableStream } from "node:stream/web";
// new ReadableStream() with no underlying source — instance still has the
// standard API (getReader, cancel, locked, ...).
const rs = new ReadableStream();
console.log("locked:", rs.locked);
console.log("getReader:", typeof rs.getReader === "function");
console.log("cancel:", typeof rs.cancel === "function");
