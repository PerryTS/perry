import { Readable } from "node:stream";
// Readable.isErrored(stream) is true after destroy(err) before close.
const r = new Readable({ read() {} });
r.on("error", () => {});
r.destroy(new Error("boom"));
console.log("isErrored:", (Readable as any).isErrored(r));
