The last four per-element key scans on the property paths now resolve through
the shape hash index.

#8936 and #8950 replaced the `js_array_get` + `js_string_key_matches` walks on
the `[[Set]]`, `delete` and `[[Get]]`-fallback paths, but four copies survived
in the plan-miss and cache-miss fallbacks:

- `js_object_get_field_by_name`'s FAST LANE — the read-plan cache's miss path,
  a full walk of up to 4096 keys. The plan is epoch-guarded and flushed on
  every GC and on descriptor / prototype / delete mutations, so this ran in
  full each time it was flushed. It put `js_string_key_matches` at **9.6% self
  time** in a computed-key read loop, second only to the entry itself.
- the write fast path's read-plan miss fallback, and
- the write tail and the read tail, whose loops walked every key (two of them
  via `js_array_get`, which additionally probes each index for a per-index
  accessor).

All four go through `keys_find_slot_by_key_ptr`: shape hash index first, raw
dense-slot scan as its own fallback and correctness backstop. The two tail
sites keep their original `js_string_key_matches` test as the gate, so the
resolver can only narrow the candidate slot, never widen what is accepted.

Interleaved A/B, min-of-15 at quiet load (node on the same host in brackets):

| loop | main | this PR | |
|---|---|---|---|
| read only | 38 ms (25) | **34 ms** | −10.5% min, −18.6% mean |
| combined overwrite | 79 ms (29) | **77 ms** | −2.5% min, −13.2% mean |
| write only | 44 ms (23) | 43 ms | −2.3% |

The read-only loop is where this lands: in the combined loop the write primes
the read plan, so the read never reaches the miss path at all.

Computed-key differential output (delete/re-add ordering, `Map`/`Set` keys,
the SSO boundary, non-ASCII, floats, negatives, 1e21) is byte-identical to
node. Suite: 2779 passed, 0 failed.
