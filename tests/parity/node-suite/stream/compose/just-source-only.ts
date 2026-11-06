import { compose, Readable } from "node:stream";
// compose(src) — single-source compose returns Duplex wrapping it.
const src = Readable.from(["a", "b"]);
const composed: any = compose(src);
const out: string[] = [];
composed.on("data", (c: any) => out.push(String(c)));
composed.on("end", () => console.log("data:", out.join(",")));
