import { Readable } from "node:stream";
// emit('error', err) with a listener — does NOT throw synchronously.
const r = new Readable({ read() {} });
r.on("error", () => {});
let thrown: any = null;
try {
  r.emit("error", new Error("with-listener"));
} catch (e: any) {
  thrown = e && e.message;
}
console.log("threw:", thrown !== null);
