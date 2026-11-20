import { Readable } from "node:stream";
import { finished } from "node:stream/promises";
// finished() resolves (no value) on normal end.
const r = Readable.from(["a", "b"]);
r.on("data", () => {});
const result = await finished(r);
console.log("resolved to:", result);
console.log("is undefined:", result === undefined);
