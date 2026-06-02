import { Readable, PassThrough } from "node:stream";
import { EventEmitter } from "node:events";
// wrap() — wrap an old-style EE emitter as a Readable, then pipe it.
const old: any = new EventEmitter();
old.pause = () => {};
old.resume = () => {};
const r = new Readable({ read() {} }).wrap(old);
const dst = new PassThrough();
const out: string[] = [];
dst.on("data", (c) => out.push(String(c)));
dst.on("end", () => console.log("piped:", out.join(",")));
r.pipe(dst);
setImmediate(() => {
  old.emit("data", "x");
  old.emit("end");
});
