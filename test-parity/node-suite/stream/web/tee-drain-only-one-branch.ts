import { ReadableStream } from "node:stream/web";
// tee() then drain only one branch — the other accumulates in queue (HWM allows).
const rs = new ReadableStream({
  start(c) {
    c.enqueue("a");
    c.enqueue("b");
    c.close();
  },
});
const [a, b] = rs.tee();
// Drain a only
const reader = a.getReader();
const out: string[] = [];
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  out.push(String(value));
}
console.log("a:", out.join(","));
// b is still readable
const bReader = b.getReader();
const first = await bReader.read();
console.log("b first:", first.value);
