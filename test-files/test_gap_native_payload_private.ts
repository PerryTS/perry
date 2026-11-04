// A native payload (the cell behind a crypto Hash, a zlib stream) lives in a
// runtime-private slot of its object: no reflection, copy, clone or JSON path
// may see it, and a subclass whose super() attaches one keeps working.
// (node keeps its handle in an enumerable own symbol, so a spread copy or a
// structuredClone carries it; those rows are not asserted here.)
import * as crypto from "node:crypto";
import * as zlib from "node:zlib";

const hidden = (o: object) =>
  Reflect.ownKeys(o).filter((k) => typeof k === "symbol" && String(k).includes("NativePayload")).length;

const h = crypto.createHash("sha256");
console.log("hash hidden", hidden(h), Object.getOwnPropertySymbols(h).filter((s) => String(s).includes("Native")).length);
console.log("hash keys", JSON.stringify(Object.keys(h)));
console.log("hash spread", JSON.stringify(Object.keys({ ...h })), hidden({ ...h }));
console.log("hash assign", JSON.stringify(Object.keys(Object.assign({}, h))), hidden(Object.assign({}, h)));
console.log("hash json", JSON.stringify(h));
console.log("hash in", Object.getOwnPropertyNames(h).includes("[[NativePayload]]"));
h.update("abc");
console.log("hash digest", h.digest("hex"));
class Tagged extends zlib.Gzip {
  tag: number;
  constructor() {
    super();
    this.tag = 7;
  }
}
const g = new Tagged();
console.log("sub hidden", hidden(g), g.tag, g instanceof zlib.Gzip, g instanceof Tagged);
console.log("sub own tag", Object.keys(g).includes("tag"));
const chunks: Buffer[] = [];
g.on("data", (c: Buffer) => chunks.push(c));
g.on("end", () => {
  const back = zlib.gunzipSync(Buffer.concat(chunks)).toString();
  console.log("sub roundtrip", back);
});
g.end("payload in a private slot");
