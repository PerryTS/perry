import * as stream from "node:stream";
import { Readable, Transform } from "node:stream";
// stream.compose chains multiple Transforms into one composite Duplex.
const upper = new Transform({ transform(c, _e, cb) { cb(null, String(c).toUpperCase()); } });
const exclaim = new Transform({ transform(c, _e, cb) { cb(null, String(c) + "!"); } });
const piped = (stream as any).compose(upper, exclaim);
const out: string[] = [];
piped.on("data", (c: any) => out.push(String(c)));
piped.on("end", () => console.log("joined:", out.join("")));
Readable.from(["a", "b"]).pipe(piped);
