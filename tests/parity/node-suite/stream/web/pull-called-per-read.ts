import { ReadableStream } from "node:stream/web";
// pull() is called each time the queue is empty + read requested.
let pullCount = 0;
const rs = new ReadableStream({
  pull(c) {
    pullCount++;
    if (pullCount <= 3) c.enqueue(pullCount);
    else c.close();
  },
});
const reader = rs.getReader();
while (true) {
  const { done } = await reader.read();
  if (done) break;
}
console.log("pull count:", pullCount);
