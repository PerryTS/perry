import * as stream from "node:stream";
import { Readable } from "node:stream";
// stream.compose with a sync generator middle stage transforms each
// upstream chunk via yield.
const piped = (stream as any).compose(function* (src: Iterable<any>) {
  for (const c of src) yield String(c) + "*";
});
const out: string[] = [];
piped.on("data", (c: any) => out.push(String(c)));
piped.on("end", () => console.log("joined:", out.join("")));
Readable.from(["x", "y"]).pipe(piped);
