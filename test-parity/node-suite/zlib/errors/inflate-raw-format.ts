import { deflateRawSync, inflateSync } from "node:zlib";

// inflateSync expects zlib format (with 2-byte header). Feeding raw deflate
// output should throw because the header check fails.
const rawBytes = deflateRawSync(Buffer.from("hello"));
try {
  inflateSync(rawBytes);
  console.log("did not throw");
} catch (e) {
  console.log("threw");
}
