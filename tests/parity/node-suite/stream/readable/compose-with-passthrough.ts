import { Readable, PassThrough, compose } from "node:stream";
// compose() with a PassThrough — should yield the source data unchanged.
const src = Readable.from(["a", "b"]);
const pt = new PassThrough();
const composed: any = compose(src, pt);
const out: string[] = [];
composed.on("data", (c: any) => out.push(String(c)));
composed.on("end", () => console.log("composed-pt:", out.join(",")));
