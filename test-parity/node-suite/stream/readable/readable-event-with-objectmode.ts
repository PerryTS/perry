import { Readable } from "node:stream";
// 'readable' event with objectMode — each read() returns one object.
const r = new Readable({
  objectMode: true,
  read() {
    this.push({ id: 1 });
    this.push({ id: 2 });
    this.push(null);
  },
});
r.on("readable", () => {
  const a = r.read();
  const b = r.read();
  const c = r.read();
  console.log("a:", a && a.id);
  console.log("b:", b && b.id);
  console.log("c:", c);
});
