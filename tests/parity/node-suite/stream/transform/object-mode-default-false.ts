import { Transform } from "node:stream";
// Transform's default objectMode is false (both sides).
const t = new Transform({ transform(c, _e, cb) { cb(null, c); } });
console.log("readableObjectMode:", t.readableObjectMode);
console.log("writableObjectMode:", t.writableObjectMode);
console.log("both false:", !t.readableObjectMode && !t.writableObjectMode);
