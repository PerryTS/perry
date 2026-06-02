import { unzipSync } from "node:zlib";

// Random bytes: no valid gzip magic (so dispatch picks zlib path) and not a
// valid zlib header either — must throw.
try {
  unzipSync(Buffer.from([0xde, 0xad, 0xbe, 0xef, 0xff, 0xff]));
  console.log("did not throw");
} catch (e) {
  console.log("threw");
}
