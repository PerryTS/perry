import { ReadableStream } from "node:stream/web";
// pull() returning a Promise paces the enqueue — the stream waits for
// pull's Promise to resolve before requesting the next pull.
let pulled = 0;
const rs = new ReadableStream({
  async pull(c) {
    pulled++;
    await new Promise((resolve) => setTimeout(resolve, 5));
    c.enqueue(pulled);
    if (pulled >= 3) c.close();
  },
});
const reader = rs.getReader();
const out: number[] = [];
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  out.push(value);
}
console.log("pulled:", out.join(","));
