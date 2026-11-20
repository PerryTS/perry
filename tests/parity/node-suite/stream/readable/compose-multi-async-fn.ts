import { Readable, Transform } from "node:stream";
// Chained .compose() with multiple async transforms.
const r = Readable.from(["ab", "cd"]);
const upper = new Transform({ transform(c, _e, cb) { cb(null, String(c).toUpperCase()); } });
const reverse = new Transform({ transform(c, _e, cb) { cb(null, String(c).split("").reverse().join("")); } });
const composed: any = (r as any).compose(upper).compose(reverse);
const out: string[] = [];
composed.on("data", (c: any) => out.push(String(c)));
composed.on("end", () => console.log("chained:", out.join(",")));
