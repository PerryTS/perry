import { Readable } from "node:stream";
// destroy(err) — verify error event fires with the supplied error.
// (The user `destroy(err, cb)` 2-arg form is not on the public Readable
// API; cb runs via the 'close' event listener.)
const r = new Readable({ read() {} });
let seen: Error | null = null;
r.on("error", (err) => (seen = err));
r.destroy(new Error("boom"));
r.on("close", () => {
  console.log("error event fired with:", seen ? (seen as Error).message : null);
  console.log("destroyed:", r.destroyed);
});
