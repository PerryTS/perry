import { compose, Readable, Duplex } from "node:stream";
// compose() with a single stream returns a Duplex wrapping it.
const r = Readable.from(["x"]);
const composed: any = compose(r);
console.log("instanceof Duplex:", composed instanceof Duplex);
const out: string[] = [];
composed.on("data", (c: any) => out.push(String(c)));
composed.on("end", () => console.log("out:", out.join(",")));
