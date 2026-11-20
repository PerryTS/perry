import { Readable } from "node:stream";
// pause(), resume(), destroy() all return the stream itself for chaining.
const r = new Readable({ read() {} });
console.log("pause:", r.pause() === r);
console.log("resume:", r.resume() === r);
r.on("error", () => {});
console.log("destroy:", r.destroy() === r);
