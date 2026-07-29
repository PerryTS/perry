import * as zlib from "node:zlib";

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
