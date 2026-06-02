import { Readable } from "node:stream";
import { finished } from "node:stream/promises";
// finished() on a stream that's already ended — resolves immediately.
const r = Readable.from(["x"]);
const collected: any[] = [];
r.on("data", (c) => collected.push(c));
await new Promise((resolve) => r.on("end", resolve));
// Now the stream has ended
let resolved = false;
finished(r).then(() => (resolved = true));
await new Promise((r) => setImmediate(r));
console.log("finished resolved on already-ended:", resolved);
