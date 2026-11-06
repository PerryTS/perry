import { Readable, Transform } from "node:stream";
// readable.compose(t1).compose(t2) chains transforms (compose() returns Duplex).
const r = Readable.from(["a", "b"]);
const up = new Transform({ transform(c, _e, cb) { cb(null, String(c).toUpperCase()); } });
const exclaim = new Transform({ transform(c, _e, cb) { cb(null, String(c) + "!"); } });
const chained: any = (r as any).compose(up).compose(exclaim);
const out: string[] = [];
chained.on("data", (c: any) => out.push(String(c)));
chained.on("end", () => console.log("chained:", out.join(",")));
