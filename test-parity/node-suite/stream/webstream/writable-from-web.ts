import { Writable } from "node:stream";
// Writable.fromWeb(web-writable) wraps a Web WritableStream as a node Writable
// and forwards write()/end() chunks.
const chunks: string[] = [];
const web = new WritableStream({ write(c) { chunks.push(c); } });
const w = (Writable as any).fromWeb(web, { objectMode: true });
console.log("is Writable:", w instanceof Writable);
w.write("x");
w.end("y");
await new Promise((resolve) => w.on("finish", resolve));
console.log("chunks:", chunks.join(""));
