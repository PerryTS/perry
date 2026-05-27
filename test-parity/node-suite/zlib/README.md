# node:zlib granular parity suite

Focused Node.js parity cases for Perry's `node:zlib` compatibility layer,
ported from `nodejs/node` `test/parallel/test-zlib-*.js` plus Deno's
node_compat zlib tests.

Cases are deterministic: every encode/decode is a closed round-trip on a
fixed input, every raw-bytes assertion is printed in hex so byte-for-byte
diff against `node --experimental-strip-types` is reliable.

Coverage areas:
- `imports/` — module shape (named / namespace / prefixless)
- `constants/` — `zlib.constants.*` flush/return/level/strategy values
- `gzip/` — `gzipSync` / `gunzipSync` and `gzip` / `gunzip` promise round-trips
- `deflate/` — `deflateSync` / `inflateSync` zlib-format round-trips
- `raw/` — `deflateRawSync` / `inflateRawSync` raw deflate (no header)
- `unzip/` — `unzipSync` auto-detect gzip vs deflate
- `crc32/` — `zlib.crc32(buf, [seed])`
- `brotli/` — `brotliCompressSync` / `brotliDecompressSync` round-trips
- `convenience/` — `typeof` checks of class/factory exports

Compatibility target: Node 22+ / current LTS behavior.
