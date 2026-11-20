import { Readable } from "node:stream";
// Calling destroy(err) emits an 'error' event with that error.
const r = new Readable({ read() {} });
r.on("error", (e) => console.log("error:", (e as Error).message));
r.destroy(new Error("boom"));
