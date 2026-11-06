import { Readable } from "node:stream";
// Readable.from(promise) — when the promise rejects, the stream should
// emit an `error` event with the rejection reason.
const p = Promise.reject(new Error("boom"));
const r = Readable.from(p);
r.on("error", (e: any) => console.log("err:", (e && e.message) || String(e)));
r.on("data", () => {});
r.on("end", () => console.log("end"));
