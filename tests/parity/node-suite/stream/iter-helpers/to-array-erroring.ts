import { Readable } from "node:stream";
// toArray() on a stream that errors should reject the returned Promise
// with the stream's error.
const r = new Readable({
  read() {
    this.destroy(new Error("toArray-fail"));
  },
});
let caught: string | null = null;
try {
  await (r as any).toArray();
} catch (e: any) {
  caught = e && e.message;
}
console.log("rejected with:", caught);
