import { gzipSync, gunzipSync } from "node:zlib";

// A valid gzip stream chopped mid-payload must throw on decompress.
const full = gzipSync(Buffer.from("the quick brown fox jumps over the lazy dog"));
const truncated = full.subarray(0, full.length - 5);
try {
  gunzipSync(truncated);
  console.log("did not throw");
} catch (e) {
  console.log("threw");
}
