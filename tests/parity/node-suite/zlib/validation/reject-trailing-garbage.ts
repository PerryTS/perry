import * as zlib from "node:zlib";
import { inflateSync, inflateRawSync, gunzipSync } from "node:zlib";

const compressed = zlib.deflateSync("a");
const doubled = Buffer.concat([compressed, compressed]);
console.log("default:", zlib.inflateSync(doubled).toString());

try {
  zlib.inflateSync(doubled, { rejectGarbageAfterEnd: true } as any);
  console.log("reject: ok");
} catch (error: any) {
  console.log("reject:", error.name, error.code);
}

for (const value of [1, "true", null] as any[]) {
  try {
    zlib.inflateSync(compressed, { rejectGarbageAfterEnd: value } as any);
    console.log("type", String(value), "ok");
  } catch (error: any) {
    console.log("type", String(value), error.name, error.code);
  }
}

// Direct calls and captured exports must retain the same completion options.
const capturedInflate = zlib.inflateSync;
for (const [name, decode] of [["captured", capturedInflate], ["namespace", zlib.inflateSync]] as const) {
  try {
    decode(doubled, { rejectGarbageAfterEnd: true });
    console.log(name, "reject ok");
  } catch (error: any) {
    console.log(name, "reject", error.name, error.code);
  }
  console.log(name, "false", decode(doubled, { rejectGarbageAfterEnd: false }).toString());
}
try {
  inflateSync(doubled, { rejectGarbageAfterEnd: true });
  console.log("direct reject ok");
} catch (error: any) {
  console.log("direct reject", error.name, error.code);
}
const raw = zlib.deflateRawSync("a");
try {
  inflateRawSync(Buffer.concat([raw, raw]), { rejectGarbageAfterEnd: true });
  console.log("raw reject ok");
} catch (error: any) {
  console.log("raw reject", error.name, error.code);
}
console.log("raw default", inflateRawSync(Buffer.concat([raw, raw])).toString());
const gzip = zlib.gzipSync("a");
console.log("gzip members default", gunzipSync(Buffer.concat([gzip, gzip])).toString());
try {
  gunzipSync(Buffer.concat([gzip, gzip]), { rejectGarbageAfterEnd: true });
  console.log("gzip members reject ok");
} catch (error: any) {
  console.log("gzip members reject", error.name, error.code);
}

console.log("gzip members false", gunzipSync(Buffer.concat([gzip, gzip]), { rejectGarbageAfterEnd: false }).toString());
console.log("gzip padding default", gunzipSync(Buffer.concat([gzip, Buffer.from([0, 0])])).toString());
