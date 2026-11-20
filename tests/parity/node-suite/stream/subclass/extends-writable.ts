import { Writable } from "node:stream";
// Custom Writable subclass implementing _write delivers via 'finish'.
class Collect extends Writable {
  chunks: string[] = [];
  _write(c: any, _e: any, cb: any) { this.chunks.push(String(c)); cb(); }
}
const w = new Collect();
w.on("finish", () => console.log("joined:", w.chunks.join("")));
w.write("hello ");
w.end("world");
