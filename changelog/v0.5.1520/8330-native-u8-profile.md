### Native values: expose `u8` and `byte` in `perry/native` (#6827)

Applications can now use checked `u8(value)` conversions and exact one-byte
`u8` or `byte` fields in verifier-backed `pod<T>` records. Out-of-range,
fractional, negative, and non-number conversions fail instead of truncating or
wrapping.
