**Computed string properties are preserved on typed arrays.**

Computed assignments such as `u8["p" + k] = value` now use the canonical
property setter, preserving named and symbol properties while keeping numeric
byte-index writes on their existing fast path.
