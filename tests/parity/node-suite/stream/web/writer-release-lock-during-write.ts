import { WritableStream } from "node:stream/web";
// releaseLock() while a write is pending — the pending write Promise rejects.
const ws = new WritableStream({
  async write() {
    await new Promise((resolve) => setTimeout(resolve, 30));
  },
});
const w = ws.getWriter();
const writeP = w.write("x");
w.releaseLock();
let err: string | null = null;
try {
  await writeP;
} catch (e: any) {
  err = e && e.name;
}
console.log("released during write -> err name:", err);
