import { Readable } from "node:stream";
import { finished } from "node:stream/promises";
// stream/promises.finished returns a Promise — verify the type and that
// it resolves once the stream ends.
const r = Readable.from(["x"]);
r.on("data", () => {});
const p = finished(r);
console.log("is Promise:", typeof (p as any).then === "function");
await p;
console.log("resolved");
