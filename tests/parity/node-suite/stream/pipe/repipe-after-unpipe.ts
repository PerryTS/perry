import { PassThrough } from "node:stream";
// After unpipe(), pipe() can be called again with the same target — the
// destination receives data flowing through after the re-pipe.
const src = new PassThrough();
const sink = new PassThrough();
const out: string[] = [];
sink.on("data", (c) => out.push(String(c)));
src.pipe(sink);
src.unpipe(sink);
src.pipe(sink);
src.write("after-repipe");
src.end();
sink.on("end", () => console.log("joined:", out.join("")));
