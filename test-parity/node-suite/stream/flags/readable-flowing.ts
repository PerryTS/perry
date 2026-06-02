import { Readable } from "node:stream";
// readable.readableFlowing tracks the flowing mode (null/true/false).
const r = new Readable({ read() {} });
console.log("initial:", r.readableFlowing);
r.on("data", () => {});
console.log("after data listener:", r.readableFlowing);
r.pause();
console.log("after pause:", r.readableFlowing);
