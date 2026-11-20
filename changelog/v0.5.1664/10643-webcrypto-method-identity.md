### Fixed

- `globalThis.crypto.randomUUID`, `.getRandomValues`, and `crypto.subtle`'s KEM methods (`encapsulateBits`/`decapsulateBits`/`encapsulateKey`/`decapsulateKey`) now have a stable identity across reads (`crypto.randomUUID === crypto.randomUUID` is `true`) instead of allocating a fresh closure on every property access.
