import { Readable, Transform } from "node:stream";
// Readable.prototype.compose(stream) chains the source through another
// stream and returns a new Duplex (Node 17+ stream iterator helpers).
const r = Readable.from(["a", "b", "c"]);
const up = new Transform({
  transform(c, _e, cb) { cb(null, String(c).toUpperCase()); },
});
const composed: any = (r as any).compose(up);
const out: string[] = [];
composed.on("data", (c: any) => out.push(String(c)));
composed.on("end", () => console.log("composed via instance:", out.join(",")));
