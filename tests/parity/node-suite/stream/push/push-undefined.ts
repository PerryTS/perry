import { Readable } from "node:stream";
// push(undefined) coerces to a string in non-objectMode (or signals end);
// it should NOT crash.
const r = new Readable({ read() {} });
let errored = false;
r.on("error", () => (errored = true));
try { r.push(undefined as any); } catch { errored = true; }
console.log("pushed without crash:", !errored);
r.push(null);
