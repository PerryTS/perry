import { Readable, PassThrough } from "node:stream";
// Calling pipe() with the same destination twice should be a no-op for
// the second call (no duplicate writes).
const r = Readable.from(["a", "b", "c"]);
const out: string[] = [];
const dst = new PassThrough();
dst.on("data", (c) => out.push(String(c)));
r.pipe(dst);
r.pipe(dst); // second call — should NOT duplicate writes
dst.on("end", () => console.log("data count:", out.length, "joined:", out.join(",")));
