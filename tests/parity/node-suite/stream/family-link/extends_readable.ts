// A user class extending Readable / Transform: _read and _transform run, and
// instanceof sees the stream chain.
import { Readable, Transform } from "node:stream";
import { EventEmitter } from "node:events";

class Counter extends Readable {
  n = 0;
  _read() {
    this.n += 1;
    this.push(this.n <= 3 ? String(this.n) : null);
  }
}
class Upper extends Transform {
  _transform(chunk: Buffer, _enc: string, cb: (e: Error | null, d?: string) => void) {
    cb(null, chunk.toString().toUpperCase());
  }
}
const c = new Counter();
console.log(c instanceof Readable, c instanceof EventEmitter);
const u = new Upper();
const got: string[] = [];
u.on("data", (d: Buffer) => got.push(d.toString()));
u.on("end", () => console.log("upper", got.join("")));
c.pipe(u);
const words = new Upper();
words.on("data", (d: Buffer) => console.log("word", d.toString()));
words.end("abc");
