import { brotliDecompressSync } from "node:zlib";

// Brotli has its own framing; arbitrary bytes are invalid input.
try {
  brotliDecompressSync(Buffer.from("not a brotli stream"));
  console.log("did not throw");
} catch (e) {
  console.log("threw");
}
