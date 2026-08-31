**`JSON.parse<T[]>` now routes through the same tape gate as the generic
entry, and `bench_json_typed_roundtrip` beats node** (565 ms → 161 ms against
node's 298; identical checksums).

The schema-directed typed parse (#179 Step 1b) was written against the
pre-tape `DirectParser`. Step 2 then made the tape-based lazy parse the
generic default for exactly the payloads the specialization targets — and
nobody went back. The result inverted the feature's premise: the "fast path"
parsed 4× slower than the generic parser it claims to specialize (589 ms vs
144 ms on the benchmark's blob), and its eagerly materialized output
re-stringified 8× slower than the tape's lazy values (580 ms vs 72 ms),
because lazy tape values serialize almost directly from the tape. The
benchmark built to showcase the typed path was measuring its abandonment.

Both entries now share one `tape_route_eligible` predicate, and the typed
entry delegates to the generic one whenever the tape qualifies — licensed by
its own documented contract ("no user-visible difference from
`JSON.parse(blob) as T[]`"). The shape hint keeps the window the tape
declines: sub-1 KB payloads, above-16 MB payloads, and non-array roots. One
deliberate behavior unification rides along: `PERRY_JSON_TAPE=0/1` previously
had no effect on the typed entry; through the shared gate both entries now
honor it consistently.

The route agreement is pinned by a test that runs typed and untyped parses
over both blob-size windows under all three tape modes and a moving
collector, asserting byte-identical re-stringify output — which doubles as
node parity, since the blob is stringify output and a byte-identical
roundtrip pins field order, number formatting, and escaping through every
route.
