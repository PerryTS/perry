Calling through a `node:` namespace held as a value (`const b = { zlib };
b.zlib.createGunzip()`) now reaches `crypto.hash` and the `zlib` stream factories
(`createGzip`, `createGunzip`, `createDeflate`, …), which returned `undefined`
there while the direct call worked.
