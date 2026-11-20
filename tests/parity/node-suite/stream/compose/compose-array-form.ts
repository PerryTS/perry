import { compose, Readable, Transform, PassThrough } from "node:stream";
// compose(...streams) accepts streams as separate args — verify the
// composite shape and that data flows through.
const src = Readable.from(["a", "b"]);
const up = new Transform({
  transform(c, _e, cb) { cb(null, String(c).toUpperCase()); },
});
const sink = new PassThrough();
const composite: any = compose(src, up, sink);
const out: string[] = [];
composite.on("data", (c: any) => out.push(String(c)));
composite.on("end", () => console.log("composed via spread:", out.join(",")));
