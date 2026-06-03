import { Readable } from "node:stream";
// Readable.toWeb converts a node Readable to a WHATWG ReadableStream and
// forwards readable chunks.
const r = Readable.from(["x", "y"]);
const web = (Readable as any).toWeb(r);
console.log("is ReadableStream:", typeof web === "object" && typeof web.getReader === "function");
const reader = web.getReader();
console.log("first:", JSON.stringify(await reader.read()));
console.log("second:", JSON.stringify(await reader.read()));
console.log("done:", JSON.stringify(await reader.read()));
