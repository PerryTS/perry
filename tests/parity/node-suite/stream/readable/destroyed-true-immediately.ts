import { Readable } from "node:stream";
// destroyed flag is true immediately (synchronously) after destroy() returns.
const r = new Readable({ read() {} });
console.log("before:", r.destroyed);
r.destroy();
console.log("after:", r.destroyed);
console.log("sync true:", r.destroyed === true);
