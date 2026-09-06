Speed up JSON parsing and stringify with bounded NEON/SSE2/word string scanning, direct Unicode escape emission, and a temporary key index for very wide objects. Preserve strict syntax validation, duplicate-key order and existing GC boundaries; prove depth one cheaply for large flat scalar arrays.

Use stack-buffer ECMAScript number formatting in JSON serialization, removing per-number temporary allocations and fixing shortest-digit tie rounding. Numeric-array stringify improves a further 20%; 1,050,129 exact double values match Node and Bun.

Bound retained parse-shape metadata to 4,096 total key slots. Against the preceding fast-path implementation, sustained 50,000-key parsing improves from 44.35 to 21.04 ms and peak RSS drops from 227 to 177 MiB. A retained-output lifetime test passes moving-GC stress.

Vectorize validation and UTF-16 length counting for long non-ASCII strings, improving the Unicode JSON parse/stringify fixture by a further 3.8× without temporary allocations. Preserve short/ASCII paths and malformed-byte semantics. Tiny scalar stringify still has an unresolved roughly 4% regression against the cache-only runtime; performance acceptance remains open.

The first quiet-M1 runtime A/B measured 7.1× faster parsing and 4.7× faster stringify of a 1 MiB ASCII string, and 1.44× faster numeric-array parsing. Tiny JSON remains essentially unchanged. The reproducible harness, full Node/Bun CPU/RSS comparisons, sustained-load caveats and validation records are in `benchmarks/json_performance/`. Parity across the complete matrix remains work in progress.
