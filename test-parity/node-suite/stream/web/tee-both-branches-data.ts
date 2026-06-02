import { ReadableStream } from "node:stream/web";
// tee() — both branches receive ALL the data chunks independently.
const rs = new ReadableStream({
  start(c) {
    c.enqueue("a");
    c.enqueue("b");
    c.close();
  },
});
const [a, b] = rs.tee();
const aOut: string[] = [];
const bOut: string[] = [];
async function drain(reader: ReadableStreamDefaultReader<any>, out: string[]) {
  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    out.push(String(value));
  }
}
await Promise.all([drain(a.getReader(), aOut), drain(b.getReader(), bOut)]);
console.log("a:", aOut.join(","));
console.log("b:", bOut.join(","));
console.log("same:", aOut.join(",") === bOut.join(","));
