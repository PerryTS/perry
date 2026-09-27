`crypto` signing now covers the remaining Node shapes that #11447 exposed (#11476).

- One-shot `crypto.sign` / `crypto.verify` (including their callback forms) now work through a CommonJS `require('crypto')` receiver or as a detached value. `js_crypto_native_dispatch` had no arms for them.
- SHA-1 and SHA-224 signatures work in every Node spelling (`sha1`, `RSA-SHA1`, `RSA-SHA1-2`, `sha1WithRSAEncryption`, `sha224`, …). The RSA PKCS#1 v1.5 helpers sign over a prehash with one scheme.
- ECDSA covers P-384 and P-521 as well as P-256, in PKCS#8 and SEC1 PEM (`crypto/ec_sign.rs`). Keys report the right `namedCurve`, so jsonwebtoken ES384/ES512 sign and verify. `generateKeyPairSync('ec', { namedCurve: 'secp384r1' | 'secp521r1' })` honours the curve. Short digests are zero-padded as OpenSSL does.
- `crypto.sign`/`crypto.verify` callbacks are delivered on a later turn, as in Node.

Gap test: `test_gap_11447_crypto_sign_followups.ts`.
