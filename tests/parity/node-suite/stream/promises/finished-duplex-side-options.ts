import { Duplex } from "node:stream";
import { finished } from "node:stream/promises";

const delay = (ms: number) => new Promise((resolve) => setTimeout(() => resolve("pending"), ms));
const state = (p: Promise<unknown>) => Promise.race([
  p.then(() => "resolved", (err: any) => `rejected:${err?.code || err?.name || "error"}`),
  delay(30),
]);

const writeOnlySettled = new Duplex({
  read() {},
  write(_chunk, _enc, cb) { cb(); },
});
const defaultAfterFinish = finished(writeOnlySettled);
const readableFalseAfterFinish = finished(writeOnlySettled, { readable: false });
writeOnlySettled.end("x");
console.log("default after finish only:", await state(defaultAfterFinish));
console.log("readable false after finish only:", await state(readableFalseAfterFinish));
defaultAfterFinish.catch(() => {});
writeOnlySettled.destroy();

const readOnlySettled = new Duplex({
  read() { this.push(null); },
  write(_chunk, _enc, cb) { cb(); },
});
const defaultAfterEnd = finished(readOnlySettled);
const writableFalseAfterEnd = finished(readOnlySettled, { writable: false });
readOnlySettled.resume();
console.log("default after end only:", await state(defaultAfterEnd));
console.log("writable false after end only:", await state(writableFalseAfterEnd));
defaultAfterEnd.catch(() => {});
readOnlySettled.destroy();
