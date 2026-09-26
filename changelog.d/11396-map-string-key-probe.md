Made string-keyed `Map` lookups decode the key once instead of once per entry
(#10697). A string key against a small map (≤8 entries) used to fall to the
generic scan, which ran the full `jsvalue_eq` per entry — symbol and bigint
probes, `is_string_like` on both operands, and a fresh decode of the probe key
every time. `map/string_probe.rs` now decodes a `STRING_TAG`/SSO key once and
compares entries by bit identity, length and bytes; any other entry tag still
goes through `jsvalue_eq`, so SameValueZero semantics are unchanged. The same
probe validates the content-hash index's candidates for larger maps, and the
typed `js_map_*_string_key` path routes through it. No new side table. On the
four-constant-key "count by category" loop this is 973 → 588 instructions per
iteration; a literal-key `get` goes 771 → 371.
