import { Duplex, Readable } from "node:stream";
// Duplex.from(readable) wraps a plain Readable as a Duplex. The resulting
// Duplex emits the source's data and is finished once it ends.
const r = Readable.from(["x", "y", "z"]);
const d: any = (Duplex as any).from(r);
const out: string[] = [];
d.on("data", (c: any) => out.push(String(c)));
d.on("end", () => console.log("duplex.from(readable):", out.join(",")));
