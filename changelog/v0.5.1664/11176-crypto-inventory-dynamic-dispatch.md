Fix `crypto.getHashes()`, `getCiphers()`, `getCurves()` and `getCipherInfo()`
returning `undefined` when the `crypto` receiver is not a statically known
`node:crypto` reference: a `let` assigned from `require`, a detached method
value, or a module held in an object. Those calls reach
`js_crypto_native_dispatch` in `perry-stdlib/src/crypto/random.rs`, which had
no arm for the four names. It now routes them to the same helpers the direct
lowering uses.

undici 8.9.0's `subresource-integrity.js` calls `crypto.getHashes()` this way
at module init and reads `.length`. Under `PERRY_NO_AUTO_OPTIMIZE=1` that made
every `require('undici')` throw "Cannot read properties of undefined (reading
'length')". This is part of #11046. The auto-optimize failure reported there is
the `Socket.read()` gap (#10908). Covered by
`test-files/test_gap_11046_crypto_inventory_dynamic_receiver.cts` and a
dispatcher unit test.
