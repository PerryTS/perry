import { Readable } from "node:stream";
// If one listener throws synchronously, OTHER listeners may still fire,
// but EE actually throws the error. Verify behavior either way.
const r = new Readable({ read() {} });
let second = 0;
r.on("custom", () => { throw new Error("first-fail"); });
r.on("custom", () => second++);
let outerErr: string | null = null;
try {
  r.emit("custom");
} catch (e: any) {
  outerErr = e && e.message;
}
console.log("outer caught:", outerErr);
console.log("second listener fired:", second);
