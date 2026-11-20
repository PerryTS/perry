import { PassThrough } from "node:stream";
import { pipeline } from "node:stream/promises";
// pipeline() promises form with array source.
const dst = new PassThrough();
const collected: string[] = [];
dst.on("data", (c) => collected.push(String(c)));
await pipeline(["a", "b", "c"], dst);
console.log("collected:", collected.join(","));
