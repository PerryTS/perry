// Regression test for `zlib.createBrotliDecompress` — axios's
// module init feature-checks `typeof zlib.createBrotliDecompress`
// and bails out at module init if the symbol is missing. We don't
// drive a real Brotli payload through the stream here (that path
// is a TODO follow-up); we just confirm the shim returns a truthy
// object so the feature-check passes.

import * as zlib from "node:zlib";

const stream = zlib.createBrotliDecompress({});
console.log(typeof zlib.createBrotliDecompress);
console.log(typeof stream === "object" ? "OK" : "FAIL");
