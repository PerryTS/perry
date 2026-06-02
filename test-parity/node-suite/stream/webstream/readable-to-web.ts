import { Readable } from "node:stream";
// Readable.toWeb converts a node Readable to a WHATWG ReadableStream.
const r = Readable.from(["x"]);
const web = (Readable as any).toWeb(r);
console.log("is ReadableStream:", typeof web === "object" && typeof web.getReader === "function");
