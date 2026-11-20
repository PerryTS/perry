// #10056: native consumers must resolve a subarray's backing and offset.
import * as zlib from "node:zlib";
import * as crypto from "node:crypto";

const source = Buffer.from("xxshared buffer bytesyy");
const view = source.subarray(2, -2);
const expected = "shared buffer bytes";
const zipped = zlib.gzipSync(view);
console.log("compressed view", zlib.gunzipSync(zipped).toString() === expected);
const framed = Buffer.concat([Buffer.from([0, 0]), zipped, Buffer.from([0])]);
console.log("compressed input view", zlib.gunzipSync(framed.subarray(2, -1)).toString() === expected);
console.log("hash view", crypto.createHash("sha256").update(view).digest("hex") ===
  crypto.createHash("sha256").update(expected).digest("hex"));
source[2] = 83;
console.log("native observes write", zlib.gunzipSync(zlib.gzipSync(view)).toString() === "Shared buffer bytes");
