import { Readable, finished } from "node:stream";
// finished(stream, { cleanup: true }, cb) — auto-removes the temporary
// listeners after the callback fires. Verify by counting listeners before
// and after.
const r = Readable.from(["x"]);
r.on("data", () => {});
const before = r.eventNames().length;
finished(r, { cleanup: true } as any, () => {
  setImmediate(() => {
    const after = r.eventNames().length;
    console.log("cleanup removed extra listeners:", after <= before);
  });
});
