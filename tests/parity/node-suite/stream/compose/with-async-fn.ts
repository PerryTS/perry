import * as stream from "node:stream";
import { Readable } from "node:stream";
// stream.compose accepts an async generator as a middle stage; the resulting
// Duplex preserves the streaming + async-iteration semantics.
const piped = (stream as any).compose(async function* (src: AsyncIterable<any>) {
  for await (const c of src) yield String(c) + "+";
});
const out: string[] = [];
piped.on("data", (c: any) => out.push(String(c)));
piped.on("end", () => console.log("joined:", out.join("")));
Readable.from(["a", "b"]).pipe(piped);
