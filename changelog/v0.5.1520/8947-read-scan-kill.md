Killed the READ path's per-element key scan — the twin of #8936's write/delete
fix: **−27% on the dynamic-property overwrite loop** (interleaved A/B pairs at
stable load: 660 → 496, 681 → 497, 680 → 497 ms; node on the same host: 55 ms).

#8936 replaced the `for i in 0..key_count { js_array_get(keys, i) +
js_string_key_matches }` walks on the `[[Set]]` and `delete` paths, but an
isolated profile of a pure overwrite loop still showed `js_array_get_f64` at
**23.5% self time** — and the caller graph attributed it to
`accessors::own_data_field_by_name`: the `[[Get]]` fallback's own copy of the
same scan, run on every dynamic string-keyed read.

It now goes through the same shared helper (`keys_find_slot_by_key_ptr`): the
shape hash index answers in O(1) when present, with the raw dense-slot linear
scan as fallback and correctness backstop. The helper's byte resolver is
SSO-aware, preserving #1781's short-key acceptance that the old loop's comment
guarded.

Suite: 2772 passed, 0 failed (full macOS run).
