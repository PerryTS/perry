`crypto.createHash`, `createHmac`, `createCipheriv` and `createDecipheriv` now
return ordinary objects with node's shape: `Object.keys(hash)` is
`["_options"]`, `JSON.stringify(cipher)` is `{"_decoder":null}`, `instanceof
crypto.Hash` (and `Hmac`, `Cipheriv`, `Decipheriv`) is true, and each object is
its own `Map`/`WeakMap` key. The hasher or cipher state is a native payload the
collector owns. `digest()`/`final()` free it at once, and an object that is
never finished frees it when collected, so a server that hashes per request no
longer holds a handle id or a registry entry for each one. Hashing loops run
about 15% fewer instructions.

The pattern is shared (`perry_runtime::native_payload`): an ordinary object
whose `native_state` word is a GC-traced payload cell with a drop that runs
exactly once and native-byte accounting. `docs/native-payload-pattern.md` has
the per-family conversion checklist (#11919).
