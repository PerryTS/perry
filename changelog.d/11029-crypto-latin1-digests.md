### Fixed

- `Hash.digest()` and `Hmac.digest()` now preserve every digest byte when
  called with `"latin1"` or its `"binary"` alias. Bytes above `0x7f` no longer
  become replacement characters, and the resulting string round-trips through
  `Buffer.from(value, "latin1")` without corruption or length changes.
