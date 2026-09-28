perf(uuid): `randomUUID()` formats without bounds checks or a UTF-8 re-validation (#10523).

#10523's main cost, one `getrandom` system call per UUID, was fixed by the
per-thread entropy cache (#11351): 120,000 UUIDs now make 944 calls, one per 128,
the batching Node uses. This removes most of what remained in user space:

- `Hyphenated::new` wrote each byte's two hex digits at a running index with a
  per-byte hyphen test, so every store was bounds-checked. A constant table of
  digit positions unrolls to straight-line stores.
- `js_crypto_random_uuid` and `js_crypto_random_uuidv7` copy the 36 bytes
  through the new `Hyphenated::as_bytes`. `as_str` validated known-ASCII bytes
  as UTF-8 on every UUID only for the string constructor to copy them.

Measured (`callgrind`, whole-process instructions ÷ UUIDs, `node:crypto`
`randomUUID()`): 1,085 → 1,005 per UUID at 200k, 1,119 → 933 at 1M. The two
hot functions fell from about 367 to 290 instructions per UUID, and the 56 of
UTF-8 validation are gone.

Tests: `hyphenated_layout_is_exact` pins the digit order and hyphen positions
byte for byte, and that `as_bytes` equals `as_str`'s bytes. No version bump.
