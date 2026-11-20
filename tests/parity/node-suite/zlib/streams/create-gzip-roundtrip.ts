import { createGzip, createGunzip, gunzipSync, gzipSync } from "node:zlib";

// Transform-stream smoke: createGzip().write(...).end(...), collect 'data'.
// Use a Promise so the test prints in a deterministic order after the stream
// drains.
const gz = createGzip();
const chunks: Uint8Array[] = [];
gz.on("data", (c: Uint8Array) => chunks.push(c));
const done = new Promise<void>((resolve) => gz.on("end", () => resolve()));
gz.end(Buffer.from("stream roundtrip"));
await done;
const compressed = Buffer.concat(chunks as any);
const roundtripped = gunzipSync(compressed).toString();
console.log("stream-gzip equal:", roundtripped === "stream roundtrip");
console.log("stream-gzip out:", roundtripped);
// Symmetric: sync-gzip → stream-gunzip.
const gun = createGunzip();
const gunChunks: Uint8Array[] = [];
gun.on("data", (c: Uint8Array) => gunChunks.push(c));
const gunDone = new Promise<void>((resolve) => gun.on("end", () => resolve()));
gun.end(gzipSync(Buffer.from("reverse roundtrip")));
await gunDone;
console.log("stream-gunzip out:", Buffer.concat(gunChunks as any).toString());
