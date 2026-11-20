import { Duplex, Readable, Writable } from "node:stream";
// Duplex.from({ readable, writable }) wires a separate Readable + Writable
// into a single Duplex.
const r = Readable.from(["hello"]);
const collected: string[] = [];
const w = new Writable({
  write(c, _e, cb) { collected.push(String(c)); cb(); },
});
const d: any = (Duplex as any).from({ readable: r, writable: w });
const out: string[] = [];
d.on("data", (c: any) => out.push(String(c)));
d.on("end", () => {
  console.log("read from r:", out.join(","));
  console.log("is Duplex:", d instanceof Duplex);
});
