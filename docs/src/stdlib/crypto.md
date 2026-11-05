# Cryptography

Perry natively implements password hashing, JWT tokens, and Ethereum cryptography.

## bcrypt

```typescript
{{#include ../../examples/stdlib/crypto/snippets.ts:bcrypt}}
```

## Argon2

```typescript
{{#include ../../examples/stdlib/crypto/snippets.ts:argon2}}
```

## JSON Web Tokens

```typescript
{{#include ../../examples/stdlib/crypto/snippets.ts:jwt}}
```

## Node.js Crypto

```typescript
{{#include ../../examples/stdlib/crypto/snippets.ts:crypto-node}}
```

### Argon2 parameters

`crypto.argon2Sync` and callback-based `crypto.argon2` include optional `secret`
(pepper) and `associatedData` in the derived key. Both accept strings, Buffers,
ArrayBuffers, TypedArrays, and DataViews; views use only their visible bytes.
Omitted, `undefined`, and empty inputs are equivalent. Async calls copy the
inputs before submission, so later mutations do not change the key.

**Compatibility limit:** Perry currently supports at most 32 bytes of
`associatedData`, imposed by its Argon2 dependency. Longer inputs throw
`RangeError` with code `ERR_OUT_OF_RANGE` synchronously, including for
`crypto.argon2`; Node supports longer inputs.

## Ethers

`ethers` has no native binding: `import { ethers } from "ethers"` compiles the
real npm package (and `@noble/curves` / `@noble/hashes`) from source. Address,
unit and hashing helpers and wallet creation match Node; ABI decoding (#13960)
and transaction / message signing (#13961) do not work yet.

```typescript
{{#include ../../examples/stdlib/crypto/snippets.ts:ethers}}
```

## Next Steps

- [Utilities](utilities.md)
- [Overview](overview.md) — All stdlib modules
