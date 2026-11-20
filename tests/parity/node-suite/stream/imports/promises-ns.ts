// #1533: node:stream exposes a `promises` namespace (used widely for
// `await pipeline(...)` / `await finished(...)`). It must be an object whose
// pipeline/finished are functions, not `undefined`.
import * as stream from "node:stream";

console.log("typeof stream.promises:", typeof stream.promises);
console.log("typeof pipeline:", typeof stream.promises.pipeline);
console.log("typeof finished:", typeof stream.promises.finished);
