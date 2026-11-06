import { Readable } from "node:stream";
// Adding a 'data' listener flips the stream into flowing mode (readableFlowing becomes true).
const r = new Readable({ read() {} });
console.log("flowing before:", r.readableFlowing);
r.on("data", () => {});
console.log("flowing after 'data':", r.readableFlowing);
