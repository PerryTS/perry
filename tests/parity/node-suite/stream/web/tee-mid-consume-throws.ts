import { ReadableStream } from "node:stream/web";
// Get a reader, read once, then try tee() — should throw (stream locked).
const rs = new ReadableStream({
  start(c) { c.enqueue("a"); c.enqueue("b"); c.close(); },
});
const reader = rs.getReader();
await reader.read(); // partial consume
let caught: string | null = null;
try {
  rs.tee();
} catch (e: any) {
  caught = e && e.name;
}
console.log("threw:", caught !== null);
console.log("name:", caught);
