import { Readable } from "node:stream";
// destroyed flag flips true after destroy() / errored / end-of-stream.
const r = new Readable({ read() {} });
console.log("before:", r.destroyed);
r.destroy();
console.log("after destroy:", r.destroyed);
