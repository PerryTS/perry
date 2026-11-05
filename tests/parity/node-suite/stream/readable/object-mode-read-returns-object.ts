import { Readable } from "node:stream";
// In objectMode, read() returns ONE object at a time (not concatenated).
const r = new Readable({
  objectMode: true,
  read() {
    this.push({ a: 1 });
    this.push({ b: 2 });
    this.push(null);
  },
});
r.on("readable", () => {
  const first = r.read();
  const second = r.read();
  console.log("first:", JSON.stringify(first));
  console.log("second:", JSON.stringify(second));
});
