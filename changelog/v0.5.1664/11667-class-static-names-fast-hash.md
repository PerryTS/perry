Class static method and accessor lookups by name hash with ahash instead of
SipHash, so a static call that misses its site guard probes each class on the
parent chain faster.
