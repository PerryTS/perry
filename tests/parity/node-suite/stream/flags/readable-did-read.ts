import { Readable } from "node:stream";
// readable.readableDidRead is true if read() has been called or data
// has been consumed via the data event (Node 16.7+ getter).
const r = Readable.from(["a", "b"]);
console.log("didRead before consume:", (r as any).readableDidRead);
const out: string[] = [];
r.on("data", (c) => out.push(String(c)));
r.on("end", () => {
  console.log("didRead after consume:", (r as any).readableDidRead);
});
