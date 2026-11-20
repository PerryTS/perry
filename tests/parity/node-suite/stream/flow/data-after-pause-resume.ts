import { Readable } from "node:stream";
// After pause then resume, the stream continues delivering data events.
const r = Readable.from(["a", "b", "c"]);
const out: string[] = [];
r.pause();
r.on("data", (c) => out.push(String(c)));
setImmediate(() => {
  r.resume();
});
r.on("end", () => {
  console.log("out:", out.join(","));
  console.log("got 3:", out.length === 3);
});
