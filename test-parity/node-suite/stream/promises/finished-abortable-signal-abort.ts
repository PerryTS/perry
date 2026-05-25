import { Readable } from "node:stream";
import { finished } from "node:stream/promises";
// finished() with a signal — abort rejects the promise.
const r = new Readable({ read() {} });
const ctrl = new AbortController();
r.on("error", () => {});
setTimeout(() => ctrl.abort(), 10);
let errName: string | null = null;
try {
  await finished(r, { signal: ctrl.signal });
} catch (e: any) {
  errName = e && e.name;
}
console.log("rejected:", errName);
