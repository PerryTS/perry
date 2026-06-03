# node:crypto granular parity suite

Focused deterministic Node.js parity coverage for Perry's `node:crypto` and WebCrypto compatibility layers.

The cases in this directory are curated from the upstream Node.js `test/parallel/test-crypto-*` files, Bun's `test/js/node/crypto/*` compatibility tests, and Deno's Node/WebCrypto coverage, then converted into small TypeScript programs that compare output byte-for-byte between Node and Perry.

## Covered areas

- Module import shapes, inventory helpers, constants, FIPS/secure heap API shapes, and method/function `name` properties.
- Hash and HMAC algorithms, encodings, streaming-style handles, digest reuse behavior, `crypto.hash()`, XOF output lengths, and legacy callable constructors.
- PBKDF2, HKDF, scrypt, random bytes/fill/int/UUID, prime generation/checking, and timing-safe equality.
- Symmetric ciphers: AES-CBC/ECB/GCM/KW, auto-padding behavior, AAD options, auth tags, SecretKey input, and `getCipherInfo()`.
- Asymmetric crypto: RSA/RSA-PSS sign/verify/encrypt/decrypt, EC P-256 signing, DH/ECDH/X25519, key generation, KeyObject/JWK import/export surrogates, and async callback APIs.
- WebCrypto: digest, HMAC, AES-CBC/CTR/GCM/KW, ECDSA/ECDH P-256, Ed25519, X25519, RSA-OAEP/RSA-PSS/RSASSA, JWK import/export, deriveBits/deriveKey, and wrap/unwrap.

## Known gaps intentionally left for follow-up PRs

- `X509Certificate` and `crypto.Certificate` / SPKAC APIs.
- Full Node stream-backed crypto transform semantics.
- Exact DER/PEM encrypted key import/export variants and OpenSSL-specific error codes.
- WebCrypto P-384/P-521, Ed448/X448, AES-OCB, ChaCha20-Poly1305, PQC/KMAC/Argon2 surfaces.
- Exact `CryptoKey`/`KeyObject.toCryptoKey()` asymmetric object identity/prototype behavior.
- OpenSSL-specific prime generation internals and exhaustive option/error validation.
