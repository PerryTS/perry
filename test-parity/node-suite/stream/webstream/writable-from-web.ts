import { Writable } from "node:stream";
// Writable.fromWeb(web-writable) wraps a Web WritableStream as a node Writable.
const web = new WritableStream({ write() {} });
const w = (Writable as any).fromWeb(web);
console.log("is Writable:", w instanceof Writable);
