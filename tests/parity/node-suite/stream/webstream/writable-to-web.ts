import { Writable } from "node:stream";
// Writable.toWeb(node-writable) converts to a WHATWG WritableStream and
// forwards writer.write() chunks.
const chunks: string[] = [];
const w = new Writable({
  objectMode: true,
  write(c, _e, cb) {
    chunks.push(c);
    cb();
  },
});
const web = (Writable as any).toWeb(w);
console.log("is WritableStream:", typeof web === "object" && typeof web.getWriter === "function");
const writer = web.getWriter();
await writer.write("x");
await writer.close();
console.log("chunks:", chunks.join(""));
