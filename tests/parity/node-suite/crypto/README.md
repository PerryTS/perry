# node:crypto granular parity suite

Focused deterministic Node.js parity coverage for Perry's `node:crypto` and WebCrypto compatibility layers.

The cases in this directory are curated from the upstream Node.js `test/parallel/test-crypto-*` files, Bun's `test/js/node/crypto/*` compatibility tests, and Deno's Node/WebCrypto coverage, then converted into small TypeScript programs that compare output byte-for-byte between Node and Perry.

## Covered areas

- Module import shapes, inventory helpers, constants, FIPS/secure heap API shapes, and method/function `name` properties.
- Hash and HMAC algorithms, encodings, streaming-style handles, digest reuse behavior, `crypto.hash()`, XOF output lengths, and legacy callable constructors.
- PBKDF2, HKDF, scrypt, random bytes/fill/int/UUID, prime generation/checking, and timing-safe equality.
- Symmetric ciphers: AES-CBC/ECB/GCM/KW, auto-padding behavior, AAD options, auth tags, SecretKey input, and `getCipherInfo()`.
- Asymmetric crypto: RSA/RSA-PSS sign/verify/encrypt/decrypt, EC P-256 signing, DH/ECDH/X25519, X25519 KEM encapsulate/decapsulate, key generation, KeyObject/JWK import/export surrogates, and async callback APIs.
- WebCrypto: digest, HMAC, AES-CBC/CTR/GCM/KW, ECDSA/ECDH P-256/P-384/P-521, Ed25519, X25519, RSA-OAEP/RSA-PSS/RSASSA, JWK import/export, deriveBits/deriveKey, and wrap/unwrap.

The ML-KEM fixtures check that `SubtleCrypto.supports` reports both bit and key
encapsulation/decapsulation for ML-KEM-512, ML-KEM-768, and ML-KEM-1024, alongside
the existing operations and their usage checks. This is focused capability
coverage, not a claim of complete PQC or Crypto parity.

## KeyObject contracts

The public fixtures cover asymmetric `KeyObject` recognition from X509 and
CryptoKey conversion, including unmarked-string rejection and custom
`Symbol.hasInstance` precedence. Secret-key binary exports must be independent
Node Buffers with encoding-aware `toString`; plain Uint8Array string conversion
and JWK exports retain their own behavior. These contracts do not imply full
Crypto module parity or change key usage or extractability rules.

## Native-handle property presence

DiffieHellman and DiffieHellmanGroup fixtures retain the original `verifyError`
values and cover dynamic-key membership, `Reflect.has`, methods and missing
properties. Genuine native handles use the existing property dispatcher before
the unchanged heap-pointer guard; cryptographic calculations are unaffected.

## KMAC parameter contracts

The KMAC fixture follows pinned Node 26.12.1 generation defaults, numeric
coercion and validation precedence: keys are byte-aligned and at least 32 bits,
JWK key material is at least four bytes, and output lengths are byte-aligned.
It explicitly asserts rejection of a 16-bit key and uses valid inputs for the
remaining sign, export, customization and verification observations. Explicit
zero-output verification still requires a valid empty signature and the original
key and usage checks; invalid-input sentinels and key-material objects do not
become successful signatures. These checks do not change key extractability or
claim full Crypto parity.

## Known gaps intentionally left for follow-up PRs

- `X509Certificate` and `crypto.Certificate` / SPKAC APIs.
- Full Node stream-backed crypto transform semantics.
- Exact DER/PEM encrypted key import/export variants and OpenSSL-specific error codes.
- WebCrypto Ed448/X448, AES-OCB, ChaCha20-Poly1305, and PQC/KMAC surfaces.
- Exact `CryptoKey`/`KeyObject.toCryptoKey()` asymmetric object identity/prototype behavior.
- OpenSSL-specific prime generation internals and exhaustive option/error validation.
