import * as stream from "node:stream";
import { Readable, Transform } from "node:stream";
// stream.compose with a single transform wraps it as a Duplex.
const upper = new Transform({ transform(c, _e, cb) { cb(null, String(c).toUpperCase()); } });
const piped = (stream as any).compose(upper);
const out: string[] = [];
piped.on("data", (c: any) => out.push(String(c)));
piped.on("end", () => console.log("joined:", out.join("")));
Readable.from(["a", "b"]).pipe(piped);
