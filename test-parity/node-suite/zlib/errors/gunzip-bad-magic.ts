import { gunzipSync } from "node:zlib";

// gzip header magic is 0x1f 0x8b. Anything else must throw.
try {
  gunzipSync(Buffer.from("not a gzip stream"));
  console.log("did not throw");
} catch (e) {
  console.log("threw");
}
